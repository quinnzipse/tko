//! `tko init` — interactive or non-interactive generation of `tko.toml`.
//!
//! Both modes funnel through [`render_toml`] so the two paths can never
//! diverge in the format they produce. No secret/token value is ever read or
//! written here — only the name of the env var that holds it.

use std::io::Write as _;
use std::path::Path;

use serde::Serialize;

use crate::config::Config;

/// Arguments accepted by `tko init`, whether supplied via flags or prompts.
#[derive(Debug, Default)]
pub struct InitArgs {
    pub kind: Option<String>,
    pub url: Option<String>,
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub token_env: Option<String>,
    pub force: bool,
    pub non_interactive: bool,
}

#[derive(Serialize)]
struct BackendToml {
    kind: String,
    url: String,
    owner: String,
    repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_env: Option<String>,
}

#[derive(Serialize)]
struct ConfigToml {
    backend: BackendToml,
}

/// Runs `tko init` against the current directory.
pub fn run(args: InitArgs) -> anyhow::Result<()> {
    let path = Path::new("tko.toml");
    if path.exists() && !args.force {
        anyhow::bail!(
            "{} already exists; pass --force to overwrite",
            path.display()
        );
    }

    let missing_required = args.url.is_none() || args.owner.is_none() || args.repo.is_none();

    let (kind, url, owner, repo, token_env) = if !missing_required {
        (
            args.kind.unwrap_or_else(|| "gitea".to_string()),
            args.url.unwrap(),
            args.owner.unwrap(),
            args.repo.unwrap(),
            args.token_env,
        )
    } else if args.non_interactive {
        let mut missing = Vec::new();
        if args.url.is_none() {
            missing.push("--url");
        }
        if args.owner.is_none() {
            missing.push("--owner");
        }
        if args.repo.is_none() {
            missing.push("--repo");
        }
        anyhow::bail!(
            "missing required flag(s) for non-interactive init: {}",
            missing.join(", ")
        );
    } else {
        prompt_for_config(&args)?
    };

    validate_kind(&kind)?;
    validate_url(&url)?;
    if owner.trim().is_empty() {
        anyhow::bail!("owner must not be empty");
    }
    if repo.trim().is_empty() {
        anyhow::bail!("repo must not be empty");
    }

    let toml_str = render_toml(&kind, &url, &owner, &repo, token_env.as_deref())?;

    // Round-trip through the real loader so the two write paths (interactive
    // and non-interactive) are guaranteed to produce something `tko` can read
    // back.
    toml::from_str::<Config>(&toml_str)
        .map_err(|e| anyhow::anyhow!("generated tko.toml failed to parse: {e}"))?;

    std::fs::write(path, &toml_str)?;
    println!("Wrote {}", path.display());
    print_welcome(&kind, token_env.as_deref());
    Ok(())
}

fn print_welcome(kind: &str, token_env: Option<&str>) {
    println!(
        r#"
      /\_/\
     ( o.o )   tko is ready!
      > ^ <    ~ tickets, tidied ~
"#
    );

    let default_token_env = match kind.to_lowercase().as_str() {
        "gitea" => "GITEA_TOKEN",
        "github" => "GITHUB_TOKEN",
        "gitlab" => "GITLAB_TOKEN",
        _ => "TKO_TOKEN",
    };
    let env_name = token_env.unwrap_or(default_token_env);

    println!("Before you start, set your backend access token. tko looks for it in order:");
    println!("  1. Environment variable: {env_name} (or TKO_TOKEN)");
    println!("  2. .env in this project's root");
    println!("  3. ~/.config/tko/secrets.env");
    println!();
    println!("The token is never stored in tko.toml — keep it out of version control.");
    println!();
    println!("Next steps:");
    println!("  tko create \"My first ticket\"   # create a ticket");
    println!("  tko list                        # see open tickets");
    println!("  tko --help                      # explore all commands");
}

fn render_toml(
    kind: &str,
    url: &str,
    owner: &str,
    repo: &str,
    token_env: Option<&str>,
) -> anyhow::Result<String> {
    let config = ConfigToml {
        backend: BackendToml {
            kind: kind.to_lowercase(),
            url: url.to_string(),
            owner: owner.to_string(),
            repo: repo.to_string(),
            token_env: token_env.map(str::to_string),
        },
    };
    Ok(toml::to_string_pretty(&config)?)
}

fn validate_kind(kind: &str) -> anyhow::Result<()> {
    match kind.to_lowercase().as_str() {
        "gitea" | "github" => Ok(()),
        other => {
            anyhow::bail!("unsupported backend kind '{other}': expected 'gitea' or 'github'")
        }
    }
}

fn validate_url(url: &str) -> anyhow::Result<()> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| {
            anyhow::anyhow!("invalid url '{url}': must start with http:// or https://")
        })?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains(char::is_whitespace) {
        anyhow::bail!("invalid url '{url}': missing or malformed host");
    }
    Ok(())
}

fn prompt_for_config(
    args: &InitArgs,
) -> anyhow::Result<(String, String, String, String, Option<String>)> {
    let kind = prompt("Backend kind", args.kind.as_deref().or(Some("gitea")))?;
    let url = loop {
        let candidate = prompt(
            "Backend URL (e.g. https://gitea.example.com)",
            args.url.as_deref(),
        )?;
        match validate_url(&candidate) {
            Ok(()) => break candidate,
            Err(e) => eprintln!("{e}"),
        }
    };
    let owner = prompt("Repo owner", args.owner.as_deref())?;
    let repo = prompt("Repo name", args.repo.as_deref())?;
    let token_env = prompt(
        "Token env var",
        args.token_env.as_deref().or(Some("TKO_TOKEN")),
    )?;
    let token_env = (!token_env.is_empty()).then_some(token_env);
    Ok((kind, url, owner, repo, token_env))
}

fn prompt(label: &str, default: Option<&str>) -> anyhow::Result<String> {
    match default {
        Some(default) => print!("{label} [{default}]: "),
        None => print!("{label}: "),
    }
    std::io::stdout().flush()?;

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let line = line.trim();

    if line.is_empty() {
        if let Some(default) = default {
            return Ok(default.to_string());
        }
        anyhow::bail!("{label} is required");
    }
    Ok(line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_url_accepts_http_and_https() {
        assert!(validate_url("https://gitea.example.com").is_ok());
        assert!(validate_url("http://localhost:3000").is_ok());
    }

    #[test]
    fn validate_url_rejects_missing_scheme() {
        assert!(validate_url("gitea.example.com").is_err());
    }

    #[test]
    fn validate_url_rejects_empty_host() {
        assert!(validate_url("https://").is_err());
    }

    #[test]
    fn validate_kind_accepts_gitea_and_github_only() {
        assert!(validate_kind("gitea").is_ok());
        assert!(validate_kind("GITEA").is_ok());
        assert!(validate_kind("github").is_ok());
        assert!(validate_kind("gitlab").is_err());
    }

    #[test]
    fn render_toml_round_trips_through_config_loader() {
        let toml_str = render_toml(
            "gitea",
            "https://gitea.example.com",
            "myorg",
            "myproject",
            Some("MY_TOKEN"),
        )
        .unwrap();
        let config: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.backend.owner, "myorg");
        assert_eq!(config.backend.repo, "myproject");
        assert_eq!(config.backend.url, "https://gitea.example.com");
        assert_eq!(config.backend.token_env.as_deref(), Some("MY_TOKEN"));
    }

    #[test]
    fn render_toml_omits_token_env_when_absent() {
        let toml_str = render_toml(
            "gitea",
            "https://gitea.example.com",
            "myorg",
            "myproject",
            None,
        )
        .unwrap();
        assert!(!toml_str.contains("token_env"));
        let config: Config = toml::from_str(&toml_str).unwrap();
        assert!(config.backend.token_env.is_none());
    }
}
