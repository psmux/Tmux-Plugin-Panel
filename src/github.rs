/// GitHub API client for discovering and searching tmux plugins.
use anyhow::Result;
use serde::Deserialize;

const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRepo {
    pub full_name: String,
    pub name: String,
    pub description: Option<String>,
    pub stargazers_count: u32,
    pub html_url: String,
    pub default_branch: Option<String>,
    pub updated_at: Option<String>,
    pub language: Option<String>,
    pub license: Option<LicenseInfo>,
    #[serde(default)]
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LicenseInfo {
    pub spdx_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    items: Vec<GitHubRepo>,
}

impl GitHubRepo {
    pub fn desc(&self) -> &str {
        self.description.as_deref().unwrap_or("")
    }
}

fn build_client() -> Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "Accept",
        "application/vnd.github+json".parse().unwrap(),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        "2022-11-28".parse().unwrap(),
    );
    headers.insert(
        "User-Agent",
        "tmuxpanel/0.1".parse().unwrap(),
    );

    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        headers.insert(
            "Authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );
    }

    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(15))
        .build()?)
}

/// Search GitHub for tmux plugins matching query.
pub async fn search_github_plugins(query: &str, per_page: u32) -> Result<Vec<GitHubRepo>> {
    let client = build_client()?;

    let full_query = if query.to_lowercase().contains("tmux") {
        format!("{} in:name,description,readme", query)
    } else {
        format!("tmux {} in:name,description,readme", query)
    };

    let resp = client
        .get(format!("{}/search/repositories", GITHUB_API))
        .query(&[
            ("q", full_query.as_str()),
            ("sort", "stars"),
            ("order", "desc"),
            ("per_page", &per_page.to_string()),
        ])
        .send()
        .await?
        .json::<SearchResponse>()
        .await?;

    Ok(resp.items)
}

/// Get info about a specific GitHub repo.
pub async fn get_repo_info(repo: &str) -> Result<GitHubRepo> {
    let client = build_client()?;
    let resp = client
        .get(format!("{}/repos/{}", GITHUB_API, repo))
        .send()
        .await?
        .json::<GitHubRepo>()
        .await?;
    Ok(resp)
}

/// Fetch the README content for a repo.
pub async fn get_repo_readme(repo: &str) -> Result<String> {
    let client = build_client()?;
    let resp = client
        .get(format!("{}/repos/{}/readme", GITHUB_API, repo))
        .header("Accept", "application/vnd.github.raw+json")
        .send()
        .await?
        .text()
        .await?;
    Ok(resp)
}
