//! The [`Backend`] trait: the abstraction every git-service issue tracker
//! implements, plus native issue-number ID resolution (`#123` / partial
//! prefix).
//!
//! A backend is a best-effort mirror onto native issue fields (title, state,
//! labels, assignee, comments, dependencies); the [`crate::ticket::Ticket`]
//! frontmatter+body remains the source of truth. `tko` never generates its
//! own ids — the canonical id is the backend's native issue number.
#![allow(dead_code)]

pub mod gitea;
pub mod github;

use time::OffsetDateTime;

use crate::error::{Error, Result};

/// Open/closed state of a native issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Closed,
}

/// A native issue as reported by a backend.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: IssueState,
    pub labels: Vec<String>,
    pub assignee: Option<String>,
}

/// Fields for creating a new native issue.
#[derive(Debug, Clone, Default)]
pub struct NewIssue {
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub assignee: Option<String>,
}

/// A partial update to a native issue's mirrored fields. `None` leaves a
/// field unchanged.
#[derive(Debug, Clone, Default)]
pub struct IssueUpdate {
    pub title: Option<String>,
    pub body: Option<String>,
    pub labels: Option<Vec<String>>,
    pub assignee: Option<Option<String>>,
}

/// A comment on a native issue.
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub id: u64,
    pub body: String,
    pub created: OffsetDateTime,
}

/// Abstracts persistence over a git-service issue tracker.
///
/// The CLI is written entirely against this trait; `GiteaBackend` and
/// `GithubBackend` implement it, with GitLab deferred behind the same
/// interface. Dependency wiring (`add_dependency` / `remove_dependency` /
/// `list_dependencies`) is best-effort: a backend that cannot represent
/// native dependencies cheaply may no-op the writes and return an empty list.
pub trait Backend {
    /// Create a new native issue and return it as persisted.
    fn create_issue(&self, new: NewIssue) -> Result<Issue>;

    /// Fetch a single issue by its native number.
    fn get_issue(&self, number: u64) -> Result<Issue>;

    /// List all issues visible to this backend.
    fn list_issues(&self) -> Result<Vec<Issue>>;

    /// Apply a partial update to an issue's mirrored fields.
    fn update_issue(&self, number: u64, update: IssueUpdate) -> Result<Issue>;

    /// Set an issue's open/closed state.
    fn set_state(&self, number: u64, state: IssueState) -> Result<()>;

    /// Add a comment to an issue.
    fn add_comment(&self, number: u64, body: &str) -> Result<Comment>;

    /// List an issue's comments.
    fn list_comments(&self, number: u64) -> Result<Vec<Comment>>;

    /// Best-effort: record that `number` depends on `depends_on`.
    fn add_dependency(&self, number: u64, depends_on: u64) -> Result<()>;

    /// Best-effort: remove a previously recorded dependency.
    fn remove_dependency(&self, number: u64, depends_on: u64) -> Result<()>;

    /// Best-effort: list an issue's native dependencies.
    fn list_dependencies(&self, number: u64) -> Result<Vec<u64>>;
}

/// Resolve a user-supplied id (`123`, `#123`, or a numeric prefix like `12`)
/// against the known issue numbers.
///
/// Matching is exact first, then unique numeric prefix, mirroring `tk`'s
/// partial-id resolution but over native issue numbers instead of
/// `tic-<hex>` strings.
pub fn resolve_id(input: &str, known: impl IntoIterator<Item = u64>) -> Result<u64> {
    let input = input.strip_prefix('#').unwrap_or(input);

    if let Ok(exact) = input.parse::<u64>() {
        let known: Vec<u64> = known.into_iter().collect();
        if known.contains(&exact) {
            return Ok(exact);
        }
        let matches: Vec<u64> = known
            .into_iter()
            .filter(|n| n.to_string().starts_with(input))
            .collect();
        return match matches.len() {
            0 => Err(Error::TicketNotFound(input.to_string())),
            1 => Ok(matches[0]),
            _ => Err(Error::AmbiguousId {
                id: input.to_string(),
                matches: matches.iter().map(u64::to_string).collect(),
            }),
        };
    }

    Err(Error::TicketNotFound(input.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_exact_match() {
        assert_eq!(resolve_id("123", [1, 12, 123, 1234]).unwrap(), 123);
    }

    #[test]
    fn resolves_hash_prefixed_exact_match() {
        assert_eq!(resolve_id("#123", [1, 12, 123, 1234]).unwrap(), 123);
    }

    #[test]
    fn resolves_unique_numeric_prefix() {
        assert_eq!(resolve_id("45", [451, 999]).unwrap(), 451);
    }

    #[test]
    fn ambiguous_prefix_errors() {
        let err = resolve_id("10", [100, 101, 999]).unwrap_err();
        match err {
            Error::AmbiguousId { id, matches } => {
                assert_eq!(id, "10");
                let mut matches = matches;
                matches.sort();
                assert_eq!(matches, vec!["100", "101"]);
            }
            other => panic!("expected AmbiguousId, got {other:?}"),
        }
    }

    #[test]
    fn exact_match_wins_over_ambiguous_prefix() {
        assert_eq!(resolve_id("1", [1, 12, 13]).unwrap(), 1);
    }

    #[test]
    fn unknown_id_errors() {
        let err = resolve_id("999", [1, 2, 3]).unwrap_err();
        assert!(matches!(err, Error::TicketNotFound(_)));
    }

    #[test]
    fn non_numeric_input_errors() {
        let err = resolve_id("abc", [1, 2, 3]).unwrap_err();
        assert!(matches!(err, Error::TicketNotFound(_)));
    }
}
