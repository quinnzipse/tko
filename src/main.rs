//! `tko` — a ticket management CLI backed by a git-service issue tracker.

mod agents_md;
mod backend;
mod cli;
mod commands;
mod config;
mod error;
mod init;
mod ticket;

use clap::Parser;

use crate::backend::Backend;
use crate::backend::gitea::GiteaBackend;
use crate::backend::github::GithubBackend;
use crate::cli::{Cli, Command};
use crate::config::{BackendKind, Config};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Command::Init {
        kind,
        url,
        owner,
        repo,
        token_env,
        force,
        non_interactive,
    } = cli.command
    {
        return init::run(init::InitArgs {
            kind,
            url,
            owner,
            repo,
            token_env,
            force,
            non_interactive,
        });
    }

    if let Command::AgentsMd { force } = cli.command {
        return agents_md::run(force);
    }

    let config = Config::discover(&std::env::current_dir()?)?;
    let token = config.resolve_token()?;
    let backend = build_backend(&config, token)?;
    commands::dispatch(backend.as_ref(), cli.json, cli.command)
}

fn build_backend(config: &Config, token: String) -> anyhow::Result<Box<dyn Backend>> {
    match config.backend.kind {
        BackendKind::Gitea => Ok(Box::new(GiteaBackend::new(config, token)?)),
        BackendKind::Github => Ok(Box::new(GithubBackend::new(config, token)?)),
        BackendKind::Gitlab => anyhow::bail!("the gitlab backend is not yet implemented"),
    }
}
