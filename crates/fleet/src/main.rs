//! Minimal read-only fleet CLI for validation and deterministic rendering.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use fleet::{BotManifest, GatewayManifest};

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(output) => {
            if !output.is_empty() {
                print!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("fleet: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<String, String> {
    let command = args
        .next()
        .ok_or_else(|| "usage: fleet <validate|render|render-gateway> <manifest>".to_owned())?;
    let path = args
        .next()
        .ok_or_else(|| "a manifest path is required".to_owned())?;
    if args.next().is_some() {
        return Err("unexpected additional arguments".to_owned());
    }
    let source = fs::read_to_string(&path).map_err(|error| format!("read {path}: {error}"))?;
    match command.as_str() {
        "validate" => {
            BotManifest::parse(&source).map_err(|error| error.to_string())?;
            Ok(format!("{}: valid", display_file_name(&path)))
        }
        "render" => BotManifest::parse(&source)
            .and_then(|manifest| manifest.render_unit())
            .map_err(|error| error.to_string()),
        "render-gateway" => GatewayManifest::parse(&source)
            .and_then(|manifest| manifest.render_unit())
            .map_err(|error| error.to_string()),
        _ => Err(format!("unknown command: {command}")),
    }
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
    }
}
