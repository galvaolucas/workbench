//! GitHub client.
//!
//! The Desk is one GraphQL round trip. Doing this over REST would be roughly
//! twenty calls — a search per relation, then a request per PR for reviews and
//! another for check runs. Here it is four aliased searches and a fragment,
//! costing a handful of points against a 5,000/hour budget.

use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use crate::config::Host;
use crate::error::{Error, Result};

#[derive(Debug, Deserialize)]
pub struct Viewer {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

pub async fn viewer(http: &Client, host: &Host, token: &str) -> Result<Viewer> {
    let res = http
        .get(host.user_url())
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?;

    let status = res.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        // Almost always SAML: the token is valid but not authorised for the org.
        // Saying so plainly beats showing an empty app.
        let body = res.text().await.unwrap_or_default();
        if body.contains("SAML") || body.contains("saml") {
            return Err(Error::GitHub(
                "this organisation requires SSO — authorise the token for it in GitHub, then reconnect".into(),
            ));
        }
        return Err(Error::GitHub(format!("403 from GitHub — {body}")));
    }
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(Error::GitHub(format!("{status} — {body}")));
    }

    Ok(res.json().await?)
}

// ---------------------------------------------------------------------------
// The Desk query
// ---------------------------------------------------------------------------

const PR_FRAGMENT: &str = r#"
fragment pr on PullRequest {
  id
  number
  title
  url
  isDraft
  additions
  deletions
  changedFiles
  createdAt
  updatedAt
  reviewDecision
  author { login avatarUrl }
  repository { nameWithOwner }
  comments { totalCount }
  commits(last: 1) {
    nodes { commit { oid statusCheckRollup { state } } }
  }
}
"#;

const DESK_QUERY: &str = r#"
query Desk($mine: String!, $reviewing: String!, $mentioned: String!, $involved: String!) {
  mine:      search(query: $mine,      type: ISSUE, first: 40) { nodes { ...pr } }
  reviewing: search(query: $reviewing, type: ISSUE, first: 40) { nodes { ...pr } }
  mentioned: search(query: $mentioned, type: ISSUE, first: 40) { nodes { ...pr } }
  involved:  search(query: $involved,  type: ISSUE, first: 40) { nodes { ...pr } }
  viewer { organizations(first: 50) { nodes { login } } }
  rateLimit { cost remaining }
}
"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    pub login: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub name_with_owner: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Count {
    pub total_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct StatusCheckRollup {
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub oid: String,
    pub status_check_rollup: Option<StatusCheckRollup>,
}

#[derive(Debug, Deserialize)]
pub struct CommitNode {
    pub commit: Commit,
}

#[derive(Debug, Deserialize)]
pub struct Commits {
    pub nodes: Vec<CommitNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub id: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub is_draft: bool,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    pub created_at: String,
    pub updated_at: String,
    pub review_decision: Option<String>,
    pub author: Option<Author>,
    pub repository: Repository,
    pub comments: Count,
    pub commits: Commits,
}

impl PullRequest {
    pub fn head_oid(&self) -> Option<&str> {
        self.commits.nodes.first().map(|n| n.commit.oid.as_str())
    }

    /// SUCCESS | FAILURE | PENDING | ERROR | EXPECTED, or None when the repo
    /// has no checks configured at all.
    pub fn checks_state(&self) -> Option<&str> {
        self.commits
            .nodes
            .first()
            .and_then(|n| n.commit.status_check_rollup.as_ref())
            .and_then(|r| r.state.as_deref())
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchResult {
    pub nodes: Vec<PullRequest>,
}

#[derive(Debug, Deserialize)]
pub struct RateLimit {
    pub cost: i64,
    pub remaining: i64,
}

#[derive(Debug, Deserialize)]
pub struct OrgNode {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct Organizations {
    pub nodes: Vec<OrgNode>,
}

#[derive(Debug, Deserialize)]
pub struct Viewer2 {
    pub organizations: Organizations,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeskData {
    pub viewer: Viewer2,
    pub mine: SearchResult,
    pub reviewing: SearchResult,
    pub mentioned: SearchResult,
    pub involved: SearchResult,
    pub rate_limit: Option<RateLimit>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

/// Everything the Desk needs, in one request.
pub async fn desk(http: &Client, host: &Host, token: &str, login: &str) -> Result<DeskData> {
    // `archived:false` keeps dead repositories out; `-author:@me` stops your own
    // PRs being counted twice, once as yours and once as merely involved.
    let variables = json!({
        "mine":      format!("is:open is:pr archived:false author:{login}"),
        "reviewing": format!("is:open is:pr archived:false review-requested:{login}"),
        "mentioned": format!("is:open is:pr archived:false mentions:{login}"),
        "involved":  format!("is:open is:pr archived:false involves:{login} -author:{login}"),
    });

    let body = json!({
        "query": format!("{DESK_QUERY}{PR_FRAGMENT}"),
        "variables": variables,
    });

    let res = http
        .post(host.graphql_url())
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;

    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::GitHub(
                "your token was rejected — sign out and reconnect".into(),
            ));
        }
        return Err(Error::GitHub(format!("{status} — {text}")));
    }

    let parsed: GraphQlResponse<DeskData> = res.json().await?;

    match parsed.data {
        // GitHub answers 200 with partial data when one org blocks the token
        // (SAML, usually). Showing the rest beats showing nothing.
        Some(data) => {
            if !parsed.errors.is_empty() {
                log::warn!(
                    "partial Desk results: {}",
                    parsed
                        .errors
                        .iter()
                        .map(|e| e.message.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            }
            Ok(data)
        }
        None => Err(Error::GitHub(
            parsed
                .errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        )),
    }
}
