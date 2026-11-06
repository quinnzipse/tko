//! [`GithubBackend`]: a [`Backend`] implementation against the GitHub REST
//! API (github.com or GitHub Enterprise Server).
//!
//! Works like the Gitea backend: fields are mirrored onto native issue
//! fields on a best-effort basis. Labels are passed by name (GitHub creates
//! them on demand), pull requests are filtered out of issue listings, and
//! dependencies map onto GitHub's "blocked by" issue relationships.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{Backend, Comment, Issue, IssueState, IssueUpdate, NewIssue};
use crate::config::Config;
use crate::error::{Error, Result};

const PER_PAGE: u32 = 100;
const API_VERSION: &str = "2022-11-28";

/// Talks to a single GitHub repository's issue tracker over its REST API.
pub struct GithubBackend {
    client: reqwest::blocking::Client,
    api_base: String,
    token: String,
}

/// The REST API root for a configured instance URL: `api.github.com` for
/// github.com, `<url>/api/v3` for GitHub Enterprise Server.
fn api_root(url: &str) -> String {
    let url = url.trim_end_matches('/');
    let host = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or("");
    if host == "github.com" || host == "www.github.com" {
        "https://api.github.com".to_string()
    } else {
        format!("{url}/api/v3")
    }
}

impl GithubBackend {
    /// Build a backend from a parsed `tko.toml` and a resolved access token.
    pub fn new(config: &Config, token: String) -> Result<GithubBackend> {
        Self::with_api_root(&api_root(&config.backend.url), config, token)
    }

    fn with_api_root(api_root: &str, config: &Config, token: String) -> Result<GithubBackend> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("tko/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Error::Backend(e.to_string()))?;
        let api_base = format!(
            "{}/repos/{}/{}",
            api_root, config.backend.owner, config.backend.repo,
        );
        Ok(GithubBackend {
            client,
            api_base,
            token,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.api_base, path)
    }

    fn send_raw(
        &self,
        builder: reqwest::blocking::RequestBuilder,
    ) -> Result<reqwest::blocking::Response> {
        let response = builder
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .send()
            .map_err(|e| Error::Backend(e.to_string()))?;
        check_status(response)
    }

    fn send<T: for<'de> Deserialize<'de>>(
        &self,
        builder: reqwest::blocking::RequestBuilder,
    ) -> Result<T> {
        self.send_raw(builder)?
            .json::<T>()
            .map_err(|e| Error::Backend(e.to_string()))
    }

    fn send_no_body(&self, builder: reqwest::blocking::RequestBuilder) -> Result<()> {
        self.send_raw(builder).map(drop)
    }

    /// Fetch every page of a list endpoint. `path` may already carry a query.
    fn paginate<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<Vec<T>> {
        let sep = if path.contains('?') { '&' } else { '?' };
        let mut items = Vec::new();
        let mut page = 1;
        loop {
            let url = format!("{}{sep}per_page={PER_PAGE}&page={page}", self.url(path));
            let batch: Vec<T> = self.send(self.client.get(url))?;
            let got = batch.len();
            items.extend(batch);
            if got < PER_PAGE as usize {
                break;
            }
            page += 1;
        }
        Ok(items)
    }

    /// GitHub's dependency endpoints identify issues by internal id, not number.
    fn internal_id(&self, number: u64) -> Result<u64> {
        let issue: ApiIssue = self.send(self.client.get(self.url(&format!("/issues/{number}"))))?;
        Ok(issue.id)
    }
}

fn check_status(response: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
    if response.status().is_success() {
        Ok(response)
    } else {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        Err(Error::Backend(format!("github returned {status}: {body}")))
    }
}

impl Backend for GithubBackend {
    fn create_issue(&self, new: NewIssue) -> Result<Issue> {
        let body = CreateIssueBody {
            title: &new.title,
            body: &new.body,
            labels: &new.labels,
            assignees: new.assignee.iter().cloned().collect(),
        };
        let issue: ApiIssue = self.send(self.client.post(self.url("/issues")).json(&body))?;
        Ok(issue.into())
    }

    fn get_issue(&self, number: u64) -> Result<Issue> {
        let issue: ApiIssue = self.send(self.client.get(self.url(&format!("/issues/{number}"))))?;
        Ok(issue.into())
    }

    fn list_issues(&self) -> Result<Vec<Issue>> {
        let issues: Vec<ApiIssue> = self.paginate("/issues?state=all")?;
        Ok(issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .map(Issue::from)
            .collect())
    }

    fn update_issue(&self, number: u64, update: IssueUpdate) -> Result<Issue> {
        let body = EditIssueBody {
            title: update.title,
            body: update.body,
            labels: update.labels,
            assignees: update.assignee.map(|a| a.into_iter().collect()),
            state: None,
        };
        let issue: ApiIssue = self.send(
            self.client
                .patch(self.url(&format!("/issues/{number}")))
                .json(&body),
        )?;
        Ok(issue.into())
    }

    fn set_state(&self, number: u64, state: IssueState) -> Result<()> {
        let body = EditIssueBody {
            state: Some(match state {
                IssueState::Open => "open",
                IssueState::Closed => "closed",
            }),
            ..EditIssueBody::default()
        };
        self.send_no_body(
            self.client
                .patch(self.url(&format!("/issues/{number}")))
                .json(&body),
        )
    }

    fn add_comment(&self, number: u64, body: &str) -> Result<Comment> {
        let payload = CreateCommentBody { body };
        let comment: ApiComment = self.send(
            self.client
                .post(self.url(&format!("/issues/{number}/comments")))
                .json(&payload),
        )?;
        comment.try_into()
    }

    fn list_comments(&self, number: u64) -> Result<Vec<Comment>> {
        let comments: Vec<ApiComment> = self.paginate(&format!("/issues/{number}/comments"))?;
        comments.into_iter().map(TryInto::try_into).collect()
    }

    fn add_dependency(&self, number: u64, depends_on: u64) -> Result<()> {
        let body = BlockedByBody {
            issue_id: self.internal_id(depends_on)?,
        };
        self.send_no_body(
            self.client
                .post(self.url(&format!("/issues/{number}/dependencies/blocked_by")))
                .json(&body),
        )
    }

    fn remove_dependency(&self, number: u64, depends_on: u64) -> Result<()> {
        let id = self.internal_id(depends_on)?;
        self.send_no_body(
            self.client
                .delete(self.url(&format!("/issues/{number}/dependencies/blocked_by/{id}"))),
        )
    }

    fn list_dependencies(&self, number: u64) -> Result<Vec<u64>> {
        let issues: Vec<ApiIssue> =
            self.paginate(&format!("/issues/{number}/dependencies/blocked_by"))?;
        Ok(issues.into_iter().map(|i| i.number).collect())
    }
}

#[derive(Debug, Deserialize)]
struct ApiLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct ApiIssue {
    id: u64,
    number: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
    state: String,
    #[serde(default)]
    labels: Vec<ApiLabel>,
    assignee: Option<ApiUser>,
    #[serde(default)]
    pull_request: Option<serde_json::Value>,
}

impl From<ApiIssue> for Issue {
    fn from(issue: ApiIssue) -> Self {
        Issue {
            number: issue.number,
            title: issue.title,
            body: issue.body.unwrap_or_default(),
            state: if issue.state == "closed" {
                IssueState::Closed
            } else {
                IssueState::Open
            },
            labels: issue.labels.into_iter().map(|l| l.name).collect(),
            assignee: issue.assignee.map(|a| a.login),
        }
    }
}

#[derive(Debug, Serialize)]
struct CreateIssueBody<'a> {
    title: &'a str,
    body: &'a str,
    labels: &'a [String],
    assignees: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
struct EditIssueBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    labels: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assignees: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'static str>,
}

#[derive(Debug, Deserialize)]
struct ApiComment {
    id: u64,
    body: String,
    created_at: String,
}

impl TryFrom<ApiComment> for Comment {
    type Error = Error;

    fn try_from(comment: ApiComment) -> Result<Self> {
        let created = OffsetDateTime::parse(&comment.created_at, &Rfc3339)
            .map_err(|e| Error::Backend(format!("invalid comment timestamp: {e}")))?;
        Ok(Comment {
            id: comment.id,
            body: comment.body,
            created,
        })
    }
}

#[derive(Debug, Serialize)]
struct CreateCommentBody<'a> {
    body: &'a str,
}

#[derive(Debug, Serialize)]
struct BlockedByBody {
    issue_id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;

    fn test_backend(server_url: &str) -> GithubBackend {
        let config = Config {
            backend: crate::config::BackendConfig {
                kind: crate::config::BackendKind::Github,
                url: "https://github.com".to_string(),
                owner: "myorg".to_string(),
                repo: "myproject".to_string(),
                token_env: None,
            },
            root: std::path::PathBuf::from("."),
        };
        GithubBackend::with_api_root(server_url, &config, "test-token".to_string()).unwrap()
    }

    const ISSUE_JSON: &str = r#"{"id":9001,"number":6,"title":"dep","body":null,"state":"open","labels":[],"assignee":null}"#;

    #[test]
    fn api_root_for_github_com_and_enterprise() {
        assert_eq!(api_root("https://github.com"), "https://api.github.com");
        assert_eq!(api_root("https://github.com/"), "https://api.github.com");
        assert_eq!(
            api_root("https://ghe.example.com/"),
            "https://ghe.example.com/api/v3"
        );
    }

    #[test]
    fn get_issue_maps_fields_and_sends_auth() {
        let mut server = mockito::Server::new();
        let backend = test_backend(&server.url());

        let mock = server
            .mock("GET", "/repos/myorg/myproject/issues/42")
            .match_header("authorization", "Bearer test-token")
            .match_header("accept", "application/vnd.github+json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"id":1,"number":42,"title":"Fix bug","body":"---\nid: 42\n---\n","state":"open","labels":[{"name":"tko:type:bug"}],"assignee":{"login":"alice"}}"#,
            )
            .create();

        let issue = backend.get_issue(42).unwrap();
        mock.assert();

        assert_eq!(issue.number, 42);
        assert_eq!(issue.state, IssueState::Open);
        assert_eq!(issue.labels, vec!["tko:type:bug".to_string()]);
        assert_eq!(issue.assignee.as_deref(), Some("alice"));
    }

    #[test]
    fn null_body_becomes_empty() {
        let mut server = mockito::Server::new();
        let backend = test_backend(&server.url());
        server
            .mock("GET", "/repos/myorg/myproject/issues/6")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(ISSUE_JSON)
            .create();

        assert_eq!(backend.get_issue(6).unwrap().body, "");
    }

    #[test]
    fn create_issue_sends_label_names() {
        let mut server = mockito::Server::new();
        let backend = test_backend(&server.url());

        let mock = server
            .mock("POST", "/repos/myorg/myproject/issues")
            .match_body(Matcher::PartialJson(serde_json::json!({
                "title": "New ticket",
                "labels": ["tko:type:bug", "tko:prio:1"],
                "assignees": ["alice"],
            })))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"id":5,"number":1,"title":"New ticket","body":"","state":"open","labels":[],"assignee":null}"#,
            )
            .create();

        let issue = backend
            .create_issue(NewIssue {
                title: "New ticket".to_string(),
                body: String::new(),
                labels: vec!["tko:type:bug".to_string(), "tko:prio:1".to_string()],
                assignee: Some("alice".to_string()),
            })
            .unwrap();

        mock.assert();
        assert_eq!(issue.number, 1);
    }

    #[test]
    fn list_issues_skips_pull_requests_and_paginates() {
        let mut server = mockito::Server::new();
        let backend = test_backend(&server.url());

        let mut page1: Vec<serde_json::Value> = (1..=100)
            .map(|n| {
                serde_json::json!({
                    "id": n, "number": n, "title": "t", "body": "", "state": "open",
                    "labels": [], "assignee": null
                })
            })
            .collect();
        page1[0]["pull_request"] = serde_json::json!({"url": "x"});

        let p1 = server
            .mock("GET", "/repos/myorg/myproject/issues")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("state".into(), "all".into()),
                Matcher::UrlEncoded("page".into(), "1".into()),
                Matcher::UrlEncoded("per_page".into(), "100".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&page1).unwrap())
            .create();
        let p2 = server
            .mock("GET", "/repos/myorg/myproject/issues")
            .match_query(Matcher::UrlEncoded("page".into(), "2".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[{"id":101,"number":101,"title":"t","body":"","state":"closed","labels":[],"assignee":null}]"#,
            )
            .create();

        let issues = backend.list_issues().unwrap();
        p1.assert();
        p2.assert();
        assert_eq!(issues.len(), 100);
        assert!(issues.iter().all(|i| i.number != 1));
        assert_eq!(issues.last().unwrap().state, IssueState::Closed);
    }

    #[test]
    fn set_state_closed() {
        let mut server = mockito::Server::new();
        let backend = test_backend(&server.url());

        let mock = server
            .mock("PATCH", "/repos/myorg/myproject/issues/7")
            .match_body(Matcher::Json(serde_json::json!({"state": "closed"})))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(ISSUE_JSON)
            .create();

        backend.set_state(7, IssueState::Closed).unwrap();
        mock.assert();
    }

    #[test]
    fn add_and_list_comments() {
        let mut server = mockito::Server::new();
        let backend = test_backend(&server.url());

        let add = server
            .mock("POST", "/repos/myorg/myproject/issues/3/comments")
            .match_body(Matcher::Json(serde_json::json!({"body": "hello"})))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":10,"body":"hello","created_at":"2026-01-31T10:00:00Z"}"#)
            .create();
        let comment = backend.add_comment(3, "hello").unwrap();
        add.assert();
        assert_eq!(comment.id, 10);

        let list = server
            .mock("GET", "/repos/myorg/myproject/issues/3/comments")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"id":10,"body":"hello","created_at":"2026-01-31T10:00:00Z"}]"#)
            .create();
        assert_eq!(backend.list_comments(3).unwrap().len(), 1);
        list.assert();
    }

    #[test]
    fn dependencies_use_internal_issue_ids() {
        let mut server = mockito::Server::new();
        let backend = test_backend(&server.url());

        server
            .mock("GET", "/repos/myorg/myproject/issues/6")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(ISSUE_JSON)
            .create();

        let add = server
            .mock(
                "POST",
                "/repos/myorg/myproject/issues/5/dependencies/blocked_by",
            )
            .match_body(Matcher::Json(serde_json::json!({"issue_id": 9001})))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(ISSUE_JSON)
            .create();
        backend.add_dependency(5, 6).unwrap();
        add.assert();

        let remove = server
            .mock(
                "DELETE",
                "/repos/myorg/myproject/issues/5/dependencies/blocked_by/9001",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(ISSUE_JSON)
            .create();
        backend.remove_dependency(5, 6).unwrap();
        remove.assert();

        let list = server
            .mock(
                "GET",
                "/repos/myorg/myproject/issues/5/dependencies/blocked_by",
            )
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!("[{ISSUE_JSON}]"))
            .create();
        assert_eq!(backend.list_dependencies(5).unwrap(), vec![6]);
        list.assert();
    }

    #[test]
    fn error_status_maps_to_backend_error() {
        let mut server = mockito::Server::new();
        let backend = test_backend(&server.url());

        let mock = server
            .mock("GET", "/repos/myorg/myproject/issues/999")
            .with_status(404)
            .with_body("not found")
            .create();

        let err = backend.get_issue(999).unwrap_err();
        mock.assert();
        assert!(matches!(err, Error::Backend(m) if m.contains("github")));
    }
}
