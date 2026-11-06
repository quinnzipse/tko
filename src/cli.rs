//! Command-line interface definition.
//!
//! This is the v1 command surface, mirroring `tk`. Command bodies are wired up
//! in their own tickets; this module defines the parsing skeleton and the shared
//! global flags.

use clap::{Args, Parser, Subcommand};

/// A ticket management CLI backed by a git-service issue tracker.
#[derive(Debug, Parser)]
#[command(name = "tko", version, about, long_about = None)]
pub struct Cli {
    /// Emit machine-readable JSON instead of human output.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

/// Filter and sort flags shared by the listing commands.
#[derive(Debug, Args)]
pub struct ListFilters {
    /// Filter by assignee.
    #[arg(short = 'a', long)]
    pub assignee: Option<String>,

    /// Filter by tag.
    #[arg(short = 'T', long)]
    pub tag: Option<String>,

    /// Filter by type (task|bug|feature|epic|chore).
    #[arg(short = 't', long = "type")]
    pub ticket_type: Option<String>,

    /// Sort by field (priority|created|status|title).
    #[arg(short = 's', long)]
    pub sort: Option<String>,

    /// Reverse the sort order.
    #[arg(short = 'r', long)]
    pub reverse: bool,
}

/// The v1 subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a new ticket.
    Create {
        /// Ticket title.
        title: Option<String>,

        /// Description text.
        #[arg(short = 'd', long)]
        description: Option<String>,

        /// Design notes.
        #[arg(long)]
        design: Option<String>,

        /// Acceptance criteria.
        #[arg(long)]
        acceptance: Option<String>,

        /// Type (task|bug|feature|epic|chore).
        #[arg(short = 't', long = "type")]
        ticket_type: Option<String>,

        /// Priority 0-4, 0 = highest.
        #[arg(short = 'p', long)]
        priority: Option<u8>,

        /// Assignee.
        #[arg(short = 'a', long)]
        assignee: Option<String>,

        /// External reference (e.g. gh-123, JIRA-456).
        #[arg(long = "external-ref")]
        external_ref: Option<String>,

        /// Pull/merge request reference.
        #[arg(long)]
        pr: Option<String>,

        /// Parent ticket id.
        #[arg(long)]
        parent: Option<String>,

        /// Comma-separated tags.
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
    },

    /// Display a ticket.
    Show {
        /// Ticket id (accepts 123 or #123).
        id: String,
    },

    /// List tickets.
    #[command(alias = "ls")]
    List {
        /// Filter by status (open|in_progress|closed).
        #[arg(long)]
        status: Option<String>,

        #[command(flatten)]
        filters: ListFilters,
    },

    /// List open/in_progress tickets with all dependencies resolved.
    Ready {
        #[command(flatten)]
        filters: ListFilters,
    },

    /// List open/in_progress tickets with unresolved dependencies.
    Blocked {
        #[command(flatten)]
        filters: ListFilters,
    },

    /// List recently closed tickets.
    Closed {
        /// Limit the number of results.
        #[arg(long, default_value_t = 20)]
        limit: usize,

        #[command(flatten)]
        filters: ListFilters,
    },

    /// Set a ticket to in_progress.
    Start {
        /// Ticket id.
        id: String,
    },

    /// Set a ticket to closed.
    Close {
        /// Ticket id.
        id: String,
    },

    /// Reopen a ticket (set to open).
    Reopen {
        /// Ticket id.
        id: String,
    },

    /// Update a ticket's status.
    Status {
        /// Ticket id.
        id: String,

        /// New status (open|in_progress|closed).
        status: String,
    },

    /// Open a ticket in `$EDITOR`.
    Edit {
        /// Ticket id.
        id: String,
    },

    /// Append a timestamped note to a ticket.
    AddNote {
        /// Ticket id.
        id: String,

        /// Note text (read from stdin if omitted).
        text: Option<String>,
    },

    /// Manage dependencies.
    Dep {
        #[command(subcommand)]
        command: DepCommand,
    },

    /// Remove a dependency (alias for `dep remove`).
    Undep {
        /// Ticket id.
        id: String,

        /// Dependency id to remove.
        dep_id: String,
    },

    /// Link tickets together (symmetric).
    Link {
        /// Two or more ticket ids to link.
        #[arg(required = true, num_args = 2..)]
        ids: Vec<String>,
    },

    /// Remove a link between two tickets.
    Unlink {
        /// Ticket id.
        id: String,

        /// The linked ticket id to remove.
        target_id: String,
    },

    /// Generate a project's `tko.toml`, interactively or from flags.
    Init {
        /// Backend kind (`gitea` or `github`).
        #[arg(long)]
        kind: Option<String>,

        /// Base URL of the backend instance (e.g. https://gitea.example.com).
        #[arg(long)]
        url: Option<String>,

        /// Repo owner / namespace.
        #[arg(long)]
        owner: Option<String>,

        /// Repo name.
        #[arg(long)]
        repo: Option<String>,

        /// Name of the env var holding the backend access token.
        #[arg(long = "token-env")]
        token_env: Option<String>,

        /// Overwrite an existing tko.toml.
        #[arg(long)]
        force: bool,

        /// Fail instead of prompting if any required value is missing.
        #[arg(long, alias = "yes")]
        non_interactive: bool,
    },

    /// Bootstrap or append a `tko` agent workflow section to `AGENTS.md`.
    AgentsMd {
        /// Write the section even if one is already present.
        #[arg(long)]
        force: bool,
    },
}

/// Dependency subcommands.
#[derive(Debug, Subcommand)]
pub enum DepCommand {
    /// Add a dependency: `id` depends on `dep_id`.
    Add {
        /// Ticket id.
        id: String,
        /// Dependency id.
        dep_id: String,
    },

    /// Remove a dependency.
    #[command(alias = "rm")]
    Remove {
        /// Ticket id.
        id: String,
        /// Dependency id.
        dep_id: String,
    },

    /// Show the dependency tree.
    Tree {
        /// Root ticket id; omit to show every open root ticket.
        id: Option<String>,

        /// Show every path instead of placing each ticket once at its deepest level.
        #[arg(long)]
        full: bool,
    },

    /// Check for dependency cycles.
    Check,
}
