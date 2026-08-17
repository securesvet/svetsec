use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use image::{GenericImageView, imageops::FilterType};
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

const LIST_CACHE_TTL: Duration = Duration::from_secs(60);
const BODY_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_ARTICLE_IMAGES: usize = 4;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const IMAGE_WIDTH: u32 = 32;
const IMAGE_PIXEL_HEIGHT: u32 = 32;

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
    pub images: Vec<GithubArticleImage>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GithubArticleImage {
    pub source: String,
    pub alt: String,
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
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
        let mut images = Vec::new();
        for (alt, source) in markdown_images(&markdown)
            .into_iter()
            .take(MAX_ARTICLE_IMAGES)
        {
            match self.article_image(&source, &alt).await {
                Ok(image) => images.push(image),
                Err(error) => tracing::warn!(source, %error, "could not load article image"),
            }
        }
        let article = GithubArticleBody {
            slug: slug.to_owned(),
            title: markdown_title(&markdown).unwrap_or_else(|| title_from_slug(slug)),
            markdown,
            images,
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

    async fn article_image(&self, source: &str, alt: &str) -> Result<GithubArticleImage> {
        let path = article_asset_path(source)?;
        let url = format!(
            "https://api.github.com/repos/{}/{}/contents/{path}",
            self.0.owner, self.0.repository
        );
        let bytes = self
            .request(self.0.client.get(url))
            .header(header::ACCEPT, "application/vnd.github.raw+json")
            .query(&[("ref", &self.0.branch)])
            .send()
            .await
            .context("GitHub image request failed")?
            .error_for_status()
            .context("GitHub rejected image request")?
            .bytes()
            .await
            .context("GitHub returned an invalid image")?;
        if bytes.len() > MAX_IMAGE_BYTES {
            bail!("article image is larger than 5 MiB");
        }
        rasterize_image(source, alt, &bytes)
    }
}

fn markdown_images(markdown: &str) -> Vec<(String, String)> {
    markdown
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("![")?;
            let (alt, rest) = rest.split_once("](")?;
            let source = rest.strip_suffix(')')?.trim();
            article_asset_path(source).ok()?;
            Some((alt.trim().to_owned(), source.to_owned()))
        })
        .collect()
}

fn article_asset_path(source: &str) -> Result<String> {
    if source.is_empty()
        || source.starts_with('/')
        || source.contains("..")
        || source.contains("://")
        || !source
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
    {
        bail!("invalid article image path");
    }
    let extension = source.rsplit_once('.').map(|(_, extension)| extension);
    if !matches!(extension, Some("png" | "jpg" | "jpeg" | "webp")) {
        bail!("unsupported article image format");
    }
    Ok(format!("articles/{source}"))
}

fn rasterize_image(source: &str, alt: &str, bytes: &[u8]) -> Result<GithubArticleImage> {
    let source_image = image::load_from_memory(bytes).context("unsupported article image")?;
    let (source_width, source_height) = source_image.dimensions();
    if source_width == 0 || source_height == 0 {
        bail!("empty article image");
    }
    let scale = (IMAGE_WIDTH as f64 / f64::from(source_width))
        .min(IMAGE_PIXEL_HEIGHT as f64 / f64::from(source_height))
        .min(1.0);
    let width = (f64::from(source_width) * scale).round().max(1.0) as u32;
    let height = (f64::from(source_height) * scale).round().max(1.0) as u32;
    let resized = source_image
        .resize_exact(width, height, FilterType::Nearest)
        .to_rgba8();
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for pixel in resized.pixels() {
        let alpha = u16::from(pixel[3]);
        for channel in &pixel.0[..3] {
            let blended = (u16::from(*channel) * alpha + 255 * (255 - alpha)) / 255;
            pixels.push(blended as u8);
        }
    }
    Ok(GithubArticleImage {
        source: source.to_owned(),
        alt: alt.to_owned(),
        width: width as u16,
        height: height as u16,
        pixels,
    })
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
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};

    use super::{
        article_asset_path, markdown_images, markdown_title, rasterize_image, title_from_slug,
        valid_slug,
    };

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

    #[test]
    fn local_markdown_images_are_discovered_and_validated() {
        assert_eq!(
            markdown_images("text\n![Earth](assets/earth.png)\n"),
            vec![("Earth".into(), "assets/earth.png".into())]
        );
        assert!(article_asset_path("assets/earth.png").is_ok());
        assert!(article_asset_path("../secret.png").is_err());
        assert!(article_asset_path("https://example.com/image.png").is_err());
    }

    #[test]
    fn images_are_reduced_to_rgb_terminal_pixels() {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(64, 64, Rgba([1, 2, 3, 255])));
        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        let image = rasterize_image("assets/test.png", "Test", &bytes).unwrap();
        assert_eq!((image.width, image.height), (32, 32));
        assert_eq!(image.pixels.len(), 32 * 32 * 3);
        assert_eq!(&image.pixels[..3], &[1, 2, 3]);
    }
}
