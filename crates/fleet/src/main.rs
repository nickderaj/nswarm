//! Minimal read-only fleet CLI for validation and deterministic rendering.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use fleet::{
    BotManifest, GatewayManifest, parse_secret_source, plan_repository, render_repository_tmpfiles,
    render_repository_units, validate_repository,
};

fn main() -> ExitCode {
    match run(env::args_os().skip(1)) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("fleet: {error}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str =
    "usage: fleet <check|validate|render|render-all|render-tmpfiles-all|render-gateway|plan> ...";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandName {
    Check,
    Validate,
    Render,
    RenderAll,
    RenderTmpfilesAll,
    RenderGateway,
    Plan,
}

impl CommandName {
    fn parse(value: &OsStr) -> Result<Self, String> {
        match value.to_str() {
            Some("check") => Ok(Self::Check),
            Some("validate") => Ok(Self::Validate),
            Some("render") => Ok(Self::Render),
            Some("render-all") => Ok(Self::RenderAll),
            Some("render-tmpfiles-all") => Ok(Self::RenderTmpfilesAll),
            Some("render-gateway") => Ok(Self::RenderGateway),
            Some("plan") => Ok(Self::Plan),
            _ => Err(format!("unknown command: {}", value.to_string_lossy())),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum CliCommand<'a> {
    Check {
        root: &'a Path,
    },
    Validate {
        manifest: &'a Path,
    },
    Render {
        manifest: &'a Path,
    },
    RenderDiff {
        manifest: &'a Path,
        installed: &'a Path,
    },
    RenderAll {
        root: &'a Path,
        output: &'a Path,
    },
    RenderTmpfilesAll {
        root: &'a Path,
        output: &'a Path,
    },
    RenderGateway {
        manifest: &'a Path,
    },
    Plan {
        root: &'a Path,
        host: &'a Path,
        secrets: &'a Path,
    },
}

impl<'a> CliCommand<'a> {
    fn parse(args: &'a [OsString]) -> Result<Self, String> {
        let (command_text, rest) = args.split_first().ok_or_else(|| USAGE.to_owned())?;
        let command = CommandName::parse(command_text)?;
        match (command, rest) {
            (CommandName::Check, []) => Ok(Self::Check {
                root: Path::new("."),
            }),
            (CommandName::Check, [root]) => Ok(Self::Check {
                root: Path::new(root),
            }),
            (CommandName::Validate, [manifest]) => Ok(Self::Validate {
                manifest: Path::new(manifest),
            }),
            (CommandName::Render, [manifest]) => Ok(Self::Render {
                manifest: Path::new(manifest),
            }),
            (CommandName::Render, [manifest, flag, installed]) if flag == OsStr::new("--diff") => {
                Ok(Self::RenderDiff {
                    manifest: Path::new(manifest),
                    installed: Path::new(installed),
                })
            }
            (CommandName::RenderAll, [root, output]) => Ok(Self::RenderAll {
                root: Path::new(root),
                output: Path::new(output),
            }),
            (CommandName::RenderTmpfilesAll, [root, output]) => Ok(Self::RenderTmpfilesAll {
                root: Path::new(root),
                output: Path::new(output),
            }),
            (CommandName::RenderGateway, [manifest]) => Ok(Self::RenderGateway {
                manifest: Path::new(manifest),
            }),
            (CommandName::Plan, [flag, root, host, secrets]) if flag == OsStr::new("--all") => {
                Ok(Self::Plan {
                    root: Path::new(root),
                    host: Path::new(host),
                    secrets: Path::new(secrets),
                })
            }
            (_, _) => Err(format!(
                "unexpected arguments for {}",
                command_text.to_string_lossy()
            )),
        }
    }

    fn execute(self) -> Result<String, String> {
        match self {
            Self::Check { root } => {
                let names = validate_repository(root).map_err(|error| error.to_string())?;
                Ok(format!(
                    "{} manifest(s): {}\n",
                    names.len(),
                    names.join(", ")
                ))
            }
            Self::Validate { manifest } => {
                let source = read(manifest)?;
                BotManifest::parse(&source).map_err(|error| error.to_string())?;
                Ok(format!("{}: valid", display_file_name(manifest)))
            }
            Self::Render { manifest } => {
                let source = read(manifest)?;
                BotManifest::parse(&source)
                    .and_then(|manifest| manifest.render_unit())
                    .map_err(|error| error.to_string())
            }
            Self::RenderDiff {
                manifest,
                installed,
            } => {
                let source = read(manifest)?;
                let installed = read(installed)?;
                BotManifest::parse(&source)
                    .and_then(|manifest| manifest.render_diff(&installed))
                    .map_err(|error| error.to_string())
            }
            Self::RenderAll { root, output } => {
                fs::create_dir_all(output).map_err(|error| error.to_string())?;
                let units = render_repository_units(root).map_err(|error| error.to_string())?;
                for (name, unit) in &units {
                    fs::write(output.join(format!("{name}.service")), unit)
                        .map_err(|error| error.to_string())?;
                }
                Ok(format!("{} unit(s) rendered\n", units.len()))
            }
            Self::RenderTmpfilesAll { root, output } => {
                fs::create_dir_all(output).map_err(|error| error.to_string())?;
                let entries =
                    render_repository_tmpfiles(root).map_err(|error| error.to_string())?;
                for (name, entry) in &entries {
                    fs::write(output.join(format!("nswarm-{name}.conf")), entry)
                        .map_err(|error| error.to_string())?;
                }
                Ok(format!("{} tmpfiles contract(s) rendered\n", entries.len()))
            }
            Self::RenderGateway { manifest } => {
                let source = read(manifest)?;
                GatewayManifest::parse(&source)
                    .and_then(|manifest| manifest.render_unit())
                    .map_err(|error| error.to_string())
            }
            Self::Plan {
                root,
                host,
                secrets,
            } => {
                let secrets =
                    parse_secret_source(&read(secrets)?).map_err(|error| error.to_string())?;
                plan_repository(root, host, &secrets).map_err(|error| error.to_string())
            }
        }
    }
}

fn run<I, S>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    CliCommand::parse(&args)?.execute()
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))
}

fn display_file_name(path: &Path) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::run;

    const BOT: &str = r#"
[bot]
name = "research"
crate = "research-bot"
runtime = "rust"
user = "research-agent"
prefix = "/opt/nswarm/research"
state = "/var/lib/research-agent"
data = "/var/lib/nswarm/research"
[surface]
telegram = false
socket = "/run/research/mcp.sock"
socket_group = "research-access"
ask = true
peers = ["boss-agent"]
[secrets]
allow = ["OPENROUTER_API_KEY"]
[agent]
profile = "research"
soul = "profiles/research/SOUL.md"
skills = "profiles/research/skills"
memory = "profiles/research/memory"
toolsets = ["mcp-research"]
write_approval = true
background_review = false
[sandbox]
memory_max = "1G"
cpu_quota = "100%"
deny_paths = ["/var/lib/nswarm/tutor"]
"#;

    const GATEWAY: &str = r#"
user = "hermes-gateway"
prefix = "/opt/nswarm/hermes"
state = "/var/lib/hermes-gateway"
port = 8642
revision = "v2026.8.19"
secrets_allow = ["OPENROUTER_API_KEY"]
"#;

    #[test]
    fn missing_command_is_refused() {
        let error =
            run(std::iter::empty::<std::ffi::OsString>()).expect_err("missing command must fail");
        assert!(error.contains("usage"));
    }

    #[test]
    fn cli_validates_and_renders_real_files() {
        let directory = TempDir::new().expect("temporary directory");
        let bot_path = directory.path().join("research.toml");
        let gateway_path = directory.path().join("gateway.toml");
        fs::write(&bot_path, BOT).expect("write bot fixture");
        fs::write(&gateway_path, GATEWAY).expect("write gateway fixture");

        let validated = run(["validate".to_owned(), bot_path.display().to_string()])
            .expect("validation succeeds");
        assert_eq!(validated, "research.toml: valid");

        let unit =
            run(["render".to_owned(), bot_path.display().to_string()]).expect("render succeeds");
        assert!(unit.contains("User=research-agent"));
        assert!(unit.contains("SupplementaryGroups=research-access"));
        assert!(unit.contains("UMask=0077"));
        assert!(unit.contains("IPAddressDeny=any"));
        assert!(unit.contains("IPAddressAllow=localhost"));
        let installed_path = directory.path().join("research.service");
        fs::write(&installed_path, &unit).expect("write installed fixture");
        let plan = run([
            "render".to_owned(),
            bot_path.display().to_string(),
            "--diff".to_owned(),
            installed_path.display().to_string(),
        ])
        .expect("diff succeeds");
        assert_eq!(plan, "clean\n");

        let gateway = run([
            "render-gateway".to_owned(),
            gateway_path.display().to_string(),
        ])
        .expect("gateway render succeeds");
        assert!(gateway.contains("IPAddressDeny=any"));
        assert!(gateway.contains("UMask=0077"));
    }

    #[test]
    fn cli_rejects_unknown_and_extra_arguments() {
        let extra = run(["validate", "fixture.toml", "extra"]
            .into_iter()
            .map(str::to_owned))
        .expect_err("extra argument must fail before file access");
        assert!(extra.contains("unexpected"));

        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("fixture.toml");
        fs::write(&path, BOT).expect("write fixture");
        let unknown = run(["unknown".to_owned(), path.display().to_string()])
            .expect_err("unknown command must fail");
        assert!(unknown.contains("unknown command"));

        for args in [
            vec!["check", ".", "extra"],
            vec![
                "render-gateway",
                path.to_str().expect("UTF-8 path"),
                "extra",
            ],
            vec![
                "render",
                path.to_str().expect("UTF-8 path"),
                "--wrong",
                path.to_str().expect("UTF-8 path"),
            ],
            vec!["render-all", ".", ".", "extra"],
            vec!["render-tmpfiles-all", ".", ".", "extra"],
            vec![
                "plan",
                "--wrong",
                ".",
                ".",
                path.to_str().expect("UTF-8 path"),
            ],
        ] {
            let error = run(args.into_iter().map(str::to_owned))
                .expect_err("invalid argument shape must fail before dispatch");
            assert!(error.contains("unexpected"), "unexpected error: {error}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn cli_parser_preserves_non_utf8_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let args = [
            OsString::from("validate"),
            OsString::from_vec(b"manifest-\xff.toml".to_vec()),
        ];
        let command =
            super::CliCommand::parse(&args).expect("non-UTF-8 paths are valid CLI arguments");
        let super::CliCommand::Validate { manifest } = command else {
            panic!("validate command expected");
        };
        assert_eq!(manifest.as_os_str().as_bytes(), b"manifest-\xff.toml");
    }

    #[test]
    fn check_enumerates_the_workspace_manifests() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root");
        let output =
            run(["check".to_owned(), root.display().to_string()]).expect("fleet check succeeds");
        assert_eq!(output, "2 manifest(s): gym, research\n");

        let rendered = TempDir::new().expect("temporary rendered directory");
        let output = run([
            "render-all".to_owned(),
            root.display().to_string(),
            rendered.path().display().to_string(),
        ])
        .expect("all units render");
        assert_eq!(output, "2 unit(s) rendered\n");
        assert!(rendered.path().join("gym.service").is_file());
        assert!(rendered.path().join("research.service").is_file());

        let tmpfiles = TempDir::new().expect("temporary tmpfiles directory");
        let output = run([
            "render-tmpfiles-all".to_owned(),
            root.display().to_string(),
            tmpfiles.path().display().to_string(),
        ])
        .expect("all tmpfiles contracts render");
        assert_eq!(output, "2 tmpfiles contract(s) rendered\n");
        assert_eq!(
            fs::read_to_string(tmpfiles.path().join("nswarm-gym.conf"))
                .expect("read gym tmpfiles contract"),
            "d /run/gym 2750 gym-agent gym-access - -\n"
        );
    }

    #[test]
    fn plan_all_is_manifest_derived_and_never_prints_secrets() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root");
        let host = TempDir::new().expect("temporary host root");
        let secret_directory = TempDir::new().expect("temporary secret source");
        let secret_path = secret_directory.path().join("decrypted.env");
        let synthetic_secret = "synthetic-provider-value";
        fs::write(
            &secret_path,
            format!(
                "OPENROUTER_API_KEY={synthetic_secret}\n\
                 GYM_BOT_TOKEN=synthetic-gym-token\n\
                 OWNER_TELEGRAM_ID=1001\n\
                 TIMEZONE=Europe/London\n\
                 GYM_DATA_DIR=/var/lib/nswarm/gym\n\
                 HEALTH_IMPORT_TOKEN=synthetic-health-token\n\
                 HEALTH_BIND_HOST=127.0.0.1\n\
                 HEALTH_BIND_PORT=8090\n"
            ),
        )
        .expect("write synthetic source");

        let plan = run([
            "plan".to_owned(),
            "--all".to_owned(),
            root.display().to_string(),
            host.path().display().to_string(),
            secret_path.display().to_string(),
        ])
        .expect("plan succeeds");
        assert!(plan.contains("bot: gym"));
        assert!(plan.contains("bot: research"));
        assert!(plan.contains("environment: replace (contents redacted)"));
        assert!(!plan.contains(synthetic_secret));
    }
}
