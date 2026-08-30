//! Minimal read-only fleet CLI for validation and deterministic rendering.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use fleet::{
    BotManifest, GatewayManifest, parse_secret_source, plan_repository, render_repository_units,
    validate_repository,
};

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
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

fn run(args: impl Iterator<Item = String>) -> Result<String, String> {
    let args = args.collect::<Vec<_>>();
    let (command, rest) = args.split_first().ok_or_else(|| {
        "usage: fleet <check|validate|render|render-all|render-gateway|plan> ...".to_owned()
    })?;
    match command.as_str() {
        "check" if rest.len() <= 1 => {
            let root = rest.first().map_or(".", String::as_str);
            let names = validate_repository(Path::new(root)).map_err(|error| error.to_string())?;
            Ok(format!(
                "{} manifest(s): {}\n",
                names.len(),
                names.join(", ")
            ))
        }
        "validate" if rest.len() == 1 => {
            let path = &rest[0];
            let source = read(path)?;
            BotManifest::parse(&source).map_err(|error| error.to_string())?;
            Ok(format!("{}: valid", display_file_name(path)))
        }
        "render" if rest.len() == 1 => {
            let source = read(&rest[0])?;
            BotManifest::parse(&source)
                .and_then(|manifest| manifest.render_unit())
                .map_err(|error| error.to_string())
        }
        "render" if rest.len() == 3 && rest[1] == "--diff" => {
            let source = read(&rest[0])?;
            let installed = read(&rest[2])?;
            BotManifest::parse(&source)
                .and_then(|manifest| manifest.render_diff(&installed))
                .map_err(|error| error.to_string())
        }
        "render-all" if rest.len() == 2 => {
            let output = Path::new(&rest[1]);
            fs::create_dir_all(output).map_err(|error| error.to_string())?;
            let units =
                render_repository_units(Path::new(&rest[0])).map_err(|error| error.to_string())?;
            for (name, unit) in &units {
                fs::write(output.join(format!("{name}.service")), unit)
                    .map_err(|error| error.to_string())?;
            }
            Ok(format!("{} unit(s) rendered\n", units.len()))
        }
        "render-gateway" if rest.len() == 1 => {
            let source = read(&rest[0])?;
            GatewayManifest::parse(&source)
                .and_then(|manifest| manifest.render_unit())
                .map_err(|error| error.to_string())
        }
        "plan" if rest.len() == 4 && rest[0] == "--all" => {
            let secrets =
                parse_secret_source(&read(&rest[3])?).map_err(|error| error.to_string())?;
            plan_repository(Path::new(&rest[1]), Path::new(&rest[2]), &secrets)
                .map_err(|error| error.to_string())
        }
        "check" | "validate" | "render" | "render-all" | "render-gateway" | "plan" => {
            Err(format!("unexpected arguments for {command}"))
        }
        _ => Err(format!("unknown command: {command}")),
    }
}

fn read(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("read {path}: {error}"))
}

fn display_file_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_owned(),
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
        let error = run(std::iter::empty()).expect_err("missing command must fail");
        assert!(error.contains("usage"));
    }

    #[test]
    fn cli_validates_and_renders_real_files() {
        let directory = TempDir::new().expect("temporary directory");
        let bot_path = directory.path().join("research.toml");
        let gateway_path = directory.path().join("gateway.toml");
        fs::write(&bot_path, BOT).expect("write bot fixture");
        fs::write(&gateway_path, GATEWAY).expect("write gateway fixture");

        let validated = run(["validate".to_owned(), bot_path.display().to_string()].into_iter())
            .expect("validation succeeds");
        assert_eq!(validated, "research.toml: valid");

        let unit = run(["render".to_owned(), bot_path.display().to_string()].into_iter())
            .expect("render succeeds");
        assert!(unit.contains("User=research-agent"));
        assert!(unit.contains("SupplementaryGroups=research-access"));
        assert!(unit.contains("IPAddressDeny=any"));
        assert!(unit.contains("IPAddressAllow=localhost"));
        let installed_path = directory.path().join("research.service");
        fs::write(&installed_path, &unit).expect("write installed fixture");
        let plan = run([
            "render".to_owned(),
            bot_path.display().to_string(),
            "--diff".to_owned(),
            installed_path.display().to_string(),
        ]
        .into_iter())
        .expect("diff succeeds");
        assert_eq!(plan, "clean\n");

        let gateway = run([
            "render-gateway".to_owned(),
            gateway_path.display().to_string(),
        ]
        .into_iter())
        .expect("gateway render succeeds");
        assert!(gateway.contains("IPAddressDeny=any"));
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
        let unknown = run(["unknown".to_owned(), path.display().to_string()].into_iter())
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

    #[test]
    fn check_enumerates_the_workspace_manifests() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root");
        let output = run(["check".to_owned(), root.display().to_string()].into_iter())
            .expect("fleet check succeeds");
        assert_eq!(output, "2 manifest(s): gym, research\n");

        let rendered = TempDir::new().expect("temporary rendered directory");
        let output = run([
            "render-all".to_owned(),
            root.display().to_string(),
            rendered.path().display().to_string(),
        ]
        .into_iter())
        .expect("all units render");
        assert_eq!(output, "2 unit(s) rendered\n");
        assert!(rendered.path().join("gym.service").is_file());
        assert!(rendered.path().join("research.service").is_file());
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
        ]
        .into_iter())
        .expect("plan succeeds");
        assert!(plan.contains("bot: gym"));
        assert!(plan.contains("bot: research"));
        assert!(plan.contains("environment: replace (contents redacted)"));
        assert!(!plan.contains(synthetic_secret));
    }
}
