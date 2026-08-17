use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

const LIST_CACHE_TTL: Duration = Duration::from_secs(60);
const BODY_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
pub struct GithubSource(Arc<Inner>);

struct Inner {
    client: Client,
    owner: String,
    repository: String,
    branch: String,
    token: Option<String>,
    list_cache: RwLock<Option<Cached<Vec<GithubArticle>>>>,
    body_cache: RwLock<HashMap<String, Cached<GithubArticleBody>>>,
}

struct Cached<T> {
    value: T,
    stored_at: tokio::time::Instant,
}

#[derive(Clone, Debug, Serialize)]
pub struct GithubArticle {
    pub slug: String,
    pub title_en: String,
    pub title_ru: String,
    pub published: bool,
    pub source_path: String,
    pub edit_url: String,
    pub size: u64,
    pub sha: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GithubArticleBody {
    pub slug: String,
    pub title: String,
    pub markdown: String,
}

#[derive(Deserialize)]
struct ContentsEntry {
    name: String,
    path: String,
    sha: String,
    size: u64,
    #[serde(rename = "type")]
    kind: String,
}

impl GithubSource {
    pub fn new(repository: &str, branch: String, token: Option<String>) -> Result<Self> {
        let Some((owner, repository)) = repository.split_once('/') else {
            bail!("GitHub repository must use owner/name format");
        };
        if !valid_component(owner) || !valid_component(repository) || !valid_branch(&branch) {
            bail!("invalid GitHub repository or branch");
        }
        let client = Client::builder()
            .user_agent("svetsec.ru article reader")
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(12))
            .build()?;
        Ok(Self(Arc::new(Inner {
            client,
            owner: owner.to_owned(),
            repository: repository.to_owned(),
            branch,
            token,
            list_cache: RwLock::new(None),
            body_cache: RwLock::new(HashMap::new()),
        })))
    }

    pub async fn list(&self, refresh: bool) -> Result<Vec<GithubArticle>> {
        if !refresh
            && let Some(cache) = self.0.list_cache.read().await.as_ref()
            && cache.stored_at.elapsed() < LIST_CACHE_TTL
        {
            return Ok(cache.value.clone());
        }

        let url = format!(
            "https://api.github.com/repos/{}/{}/contents/articles",
            self.0.owner, self.0.repository
        );
        let request = self
            .request(self.0.client.get(url))
            .query(&[("ref", &self.0.branch)]);
        let response = request
            .send()
            .await
            .context("GitHub article list request failed")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        let entries = response
            .error_for_status()
            .context("GitHub rejected article list request")?
            .json::<Vec<ContentsEntry>>()
            .await
            .context("GitHub returned an invalid article list")?;
        let edit_base = format!(
            "https://github.com/{}/{}/edit/{}/",
            self.0.owner, self.0.repository, self.0.branch
        );
        let mut articles = entries
            .into_iter()
            .filter(|entry| {
                entry.kind == "file" && entry.name.ends_with(".md") && !entry.name.starts_with('_')
            })
            .filter_map(|entry| {
                let slug = entry.name.strip_suffix(".md")?.to_owned();
                valid_slug(&slug).then(|| GithubArticle {
                    title_en: title_from_slug(&slug),
                    title_ru: title_from_slug(&slug),
                    slug,
                    published: true,
                    edit_url: format!("{edit_base}{}", entry.path),
                    source_path: entry.path,
                    size: entry.size,
                    sha: entry.sha,
                })
            })
            .collect::<Vec<_>>();
        articles.sort_by(|left, right| left.slug.cmp(&right.slug));
        *self.0.list_cache.write().await = Some(Cached {
            value: articles.clone(),
            stored_at: tokio::time::Instant::now(),
        });
        Ok(articles)
    }

    pub async fn article(&self, slug: &str) -> Result<GithubArticleBody> {
        if !valid_slug(slug) {
            bail!("invalid article slug");
        }
        if let Some(cache) = self.0.body_cache.read().await.get(slug)
            && cache.stored_at.elapsed() < BODY_CACHE_TTL
        {
            return Ok(cache.value.clone());
        }

        let url = format!(
            "https://api.github.com/repos/{}/{}/contents/articles/{slug}.md",
            self.0.owner, self.0.repository
        );
        let markdown = self
            .request(self.0.client.get(url))
            .header(header::ACCEPT, "application/vnd.github.raw+json")
            .query(&[("ref", &self.0.branch)])
            .send()
            .await
            .context("GitHub article request failed")?
            .error_for_status()
            .context("GitHub rejected article request")?
            .text()
            .await
            .context("GitHub returned invalid Markdown")?;
        let article = GithubArticleBody {
            slug: slug.to_owned(),
            title: markdown_title(&markdown).unwrap_or_else(|| title_from_slug(slug)),
            markdown,
        };
        self.0.body_cache.write().await.insert(
            slug.to_owned(),
            Cached {
                value: article.clone(),
                stored_at: tokio::time::Instant::now(),
            },
        );
        Ok(article)
    }

    pub fn editor_url(&self, path: Option<&str>) -> String {
        match path {
            Some(path) => format!(
                "https://github.com/{}/{}/edit/{}/{}",
                self.0.owner, self.0.repository, self.0.branch, path
            ),
            None => format!(
                "https://github.com/{}/{}/new/{}?filename=articles/new-article.md",
                self.0.owner, self.0.repository, self.0.branch
            ),
        }
    }

    fn request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let request = request.header("X-GitHub-Api-Version", "2022-11-28");
        match &self.0.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_branch(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn title_from_slug(slug: &str) -> String {
    let mut title = slug.replace(['-', '_'], " ");
    if let Some(first) = title.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    title
}

fn markdown_title(markdown: &str) -> Option<String> {
    markdown
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{markdown_title, title_from_slug, valid_slug};

    #[test]
    fn markdown_metadata_is_safe_and_human_readable() {
        assert!(valid_slug("hello-world_2"));
        assert!(!valid_slug("../secret"));
        assert_eq!(title_from_slug("hello-world"), "Hello world");
        assert_eq!(
            markdown_title("# Real title\n\nText"),
            Some("Real title".into())
        );
    }
}
