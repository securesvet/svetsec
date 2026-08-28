use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use image::{GenericImageView, imageops::FilterType};
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use svetsec_core::Language;
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
    articles_dir: Option<PathBuf>,
    list_cache: RwLock<HashMap<String, Cached<Vec<GithubArticle>>>>,
    body_cache: RwLock<HashMap<String, Cached<GithubArticleBody>>>,
    markdown_cache: RwLock<HashMap<String, Cached<(String, Language)>>>,
    asset_cache: RwLock<HashMap<String, Cached<GithubAsset>>>,
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
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GithubArticleBody {
    pub slug: String,
    pub title: String,
    pub markdown: String,
    pub images: Vec<GithubArticleImage>,
    pub labels: Vec<String>,
    pub language: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GithubArticleImage {
    pub source: String,
    pub alt: String,
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct GithubAsset {
    pub content_type: &'static str,
    pub bytes: bytes::Bytes,
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
    pub fn new(
        repository: &str,
        branch: String,
        token: Option<String>,
        articles_dir: Option<PathBuf>,
    ) -> Result<Self> {
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
        let articles_dir = articles_dir
            .map(|path| {
                path.canonicalize().with_context(|| {
                    format!("local articles directory is missing: {}", path.display())
                })
            })
            .transpose()?;
        Ok(Self(Arc::new(Inner {
            client,
            owner: owner.to_owned(),
            repository: repository.to_owned(),
            branch,
            token,
            articles_dir,
            list_cache: RwLock::new(HashMap::new()),
            body_cache: RwLock::new(HashMap::new()),
            markdown_cache: RwLock::new(HashMap::new()),
            asset_cache: RwLock::new(HashMap::new()),
        })))
    }

    pub async fn list(&self, refresh: bool, language: Language) -> Result<Vec<GithubArticle>> {
        if let Some(directory) = &self.0.articles_dir {
            return self.local_list(directory, language).await;
        }
        let cache_key = language.path_code();
        if !refresh
            && let Some(cache) = self.0.list_cache.read().await.get(cache_key)
            && cache.stored_at.elapsed() < LIST_CACHE_TTL
        {
            return Ok(cache.value.clone());
        }
        if refresh {
            self.0.list_cache.write().await.clear();
            self.0.markdown_cache.write().await.clear();
            self.0.body_cache.write().await.clear();
            self.0.asset_cache.write().await.clear();
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
        let mut articles = Vec::new();
        for directory in entries.into_iter().filter(|entry| {
            entry.kind == "dir" && !entry.name.starts_with('_') && valid_slug(&entry.name)
        }) {
            let url = format!(
                "https://api.github.com/repos/{}/{}/contents/{}",
                self.0.owner, self.0.repository, directory.path
            );
            let variants = self
                .request(self.0.client.get(url))
                .query(&[("ref", &self.0.branch)])
                .send()
                .await
                .context("GitHub article language list request failed")?
                .error_for_status()
                .context("GitHub rejected article language list request")?
                .json::<Vec<ContentsEntry>>()
                .await
                .context("GitHub returned an invalid article language list")?;
            if let Some(entry) = preferred_variant(&variants, language) {
                let slug = directory.name;
                articles.push(GithubArticle {
                    title_en: title_from_slug(&slug),
                    title_ru: title_from_slug(&slug),
                    slug,
                    published: true,
                    edit_url: format!("{edit_base}{}", entry.path),
                    source_path: entry.path.clone(),
                    size: entry.size,
                    sha: entry.sha.clone(),
                    labels: Vec::new(),
                });
            }
        }
        articles.sort_by(|left, right| left.slug.cmp(&right.slug));
        self.0.list_cache.write().await.insert(
            cache_key.to_owned(),
            Cached {
                value: articles.clone(),
                stored_at: tokio::time::Instant::now(),
            },
        );
        Ok(articles)
    }

    pub async fn article(&self, slug: &str, language: Language) -> Result<GithubArticleBody> {
        if !valid_slug(slug) {
            bail!("invalid article slug");
        }
        if self.0.articles_dir.is_none()
            && let Some(cache) = self
                .0
                .body_cache
                .read()
                .await
                .get(&cache_key(slug, language))
            && cache.stored_at.elapsed() < BODY_CACHE_TTL
        {
            return Ok(cache.value.clone());
        }

        let (source, resolved_language) = self.markdown(slug, language).await?;
        let labels = frontmatter_labels(&source);
        let markdown = markdown_body(&source).to_owned();
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
            labels,
            language: resolved_language.path_code().to_owned(),
        };
        if self.0.articles_dir.is_none() {
            self.0.body_cache.write().await.insert(
                cache_key(slug, language),
                Cached {
                    value: article.clone(),
                    stored_at: tokio::time::Instant::now(),
                },
            );
        }
        Ok(article)
    }

    pub fn editor_url(&self, path: Option<&str>) -> String {
        match path {
            Some(path) => format!(
                "https://github.com/{}/{}/edit/{}/{}",
                self.0.owner, self.0.repository, self.0.branch, path
            ),
            None => format!(
                "https://github.com/{}/{}/new/{}?filename=articles/new-article/en.md",
                self.0.owner, self.0.repository, self.0.branch
            ),
        }
    }

    pub fn is_local(&self) -> bool {
        self.0.articles_dir.is_some()
    }

    async fn markdown(&self, slug: &str, language: Language) -> Result<(String, Language)> {
        if !valid_slug(slug) {
            bail!("invalid article slug");
        }
        if let Some(directory) = &self.0.articles_dir {
            let (path, resolved_language) = local_variant(directory, slug, language).await?;
            return tokio::fs::read_to_string(&path)
                .await
                .with_context(|| format!("could not read local article: {}", path.display()))
                .map(|markdown| (markdown, resolved_language));
        }
        let key = cache_key(slug, language);
        if let Some(cache) = self.0.markdown_cache.read().await.get(&key)
            && cache.stored_at.elapsed() < BODY_CACHE_TTL
        {
            return Ok(cache.value.clone());
        }
        let (markdown, resolved_language) = self.remote_markdown(slug, language).await?;
        self.0.markdown_cache.write().await.insert(
            key,
            Cached {
                value: (markdown.clone(), resolved_language),
                stored_at: tokio::time::Instant::now(),
            },
        );
        Ok((markdown, resolved_language))
    }

    pub async fn asset(&self, source: &str) -> Result<GithubAsset> {
        let path = article_asset_path(source)?;
        if let Some(directory) = &self.0.articles_dir {
            let relative = path
                .strip_prefix("articles/")
                .context("invalid local article asset path")?;
            let file = local_file(directory, Path::new(relative)).await?;
            let bytes = tokio::fs::read(&file)
                .await
                .with_context(|| format!("could not read local image: {}", file.display()))?;
            if bytes.len() > MAX_IMAGE_BYTES {
                bail!("article image is larger than 5 MiB");
            }
            return Ok(GithubAsset {
                content_type: image_content_type(source)?,
                bytes: bytes.into(),
            });
        }
        if let Some(cache) = self.0.asset_cache.read().await.get(&path)
            && cache.stored_at.elapsed() < BODY_CACHE_TTL
        {
            return Ok(cache.value.clone());
        }
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
        let asset = GithubAsset {
            content_type: image_content_type(source)?,
            bytes,
        };
        self.0.asset_cache.write().await.insert(
            path,
            Cached {
                value: asset.clone(),
                stored_at: tokio::time::Instant::now(),
            },
        );
        Ok(asset)
    }

    fn request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let request = request.header("X-GitHub-Api-Version", "2022-11-28");
        match &self.0.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    async fn article_image(&self, source: &str, alt: &str) -> Result<GithubArticleImage> {
        let asset = self.asset(source).await?;
        rasterize_image(source, alt, &asset.bytes)
    }

    async fn remote_markdown(&self, slug: &str, language: Language) -> Result<(String, Language)> {
        let mut last_error = None;
        for candidate in [language, language.next()] {
            let url = format!(
                "https://api.github.com/repos/{}/{}/contents/articles/{slug}/{}.md",
                self.0.owner,
                self.0.repository,
                candidate.path_code()
            );
            let response = self
                .request(self.0.client.get(url))
                .header(header::ACCEPT, "application/vnd.github.raw+json")
                .query(&[("ref", &self.0.branch)])
                .send()
                .await
                .context("GitHub article request failed")?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                last_error = Some(anyhow::anyhow!("article language variant is missing"));
                continue;
            }
            let markdown = response
                .error_for_status()
                .context("GitHub rejected article request")?
                .text()
                .await
                .context("GitHub returned invalid Markdown")?;
            return Ok((markdown, candidate));
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("article is missing")))
    }

    async fn local_list(&self, directory: &Path, language: Language) -> Result<Vec<GithubArticle>> {
        let mut entries = tokio::fs::read_dir(directory)
            .await
            .with_context(|| format!("could not read {}", directory.display()))?;
        let mut articles = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !file_type.is_dir() || name.starts_with('_') || !valid_slug(&name) {
                continue;
            }
            let Some((path, resolved_language)) =
                local_variant_optional(directory, &name, language).await?
            else {
                continue;
            };
            let metadata = tokio::fs::metadata(&path).await?;
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_nanos());
            let source_path = format!("articles/{name}/{}.md", resolved_language.path_code());
            articles.push(GithubArticle {
                title_en: title_from_slug(&name),
                title_ru: title_from_slug(&name),
                edit_url: self.editor_url(Some(&source_path)),
                source_path,
                size: metadata.len(),
                sha: format!("local-{}-{modified}", metadata.len()),
                slug: name,
                published: true,
                labels: Vec::new(),
            });
        }
        articles.sort_by(|left, right| left.slug.cmp(&right.slug));
        Ok(articles)
    }
}

fn preferred_variant(entries: &[ContentsEntry], language: Language) -> Option<&ContentsEntry> {
    [language, language.next()]
        .into_iter()
        .find_map(|candidate| {
            let filename = format!("{}.md", candidate.path_code());
            entries
                .iter()
                .find(|entry| entry.kind == "file" && entry.name == filename)
        })
}

fn cache_key(slug: &str, language: Language) -> String {
    format!("{slug}:{}", language.path_code())
}

async fn local_variant(
    directory: &Path,
    slug: &str,
    language: Language,
) -> Result<(PathBuf, Language)> {
    local_variant_optional(directory, slug, language)
        .await?
        .with_context(|| format!("article {slug} has no en.md or ru.md"))
}

async fn local_variant_optional(
    directory: &Path,
    slug: &str,
    language: Language,
) -> Result<Option<(PathBuf, Language)>> {
    for candidate in [language, language.next()] {
        let relative = PathBuf::from(slug).join(format!("{}.md", candidate.path_code()));
        if tokio::fs::try_exists(directory.join(&relative)).await? {
            return local_file(directory, &relative)
                .await
                .map(|path| Some((path, candidate)));
        }
    }
    Ok(None)
}

async fn local_file(directory: &Path, relative: &Path) -> Result<PathBuf> {
    let path = tokio::fs::canonicalize(directory.join(relative))
        .await
        .with_context(|| format!("local article file is missing: {}", relative.display()))?;
    if !path.starts_with(directory) {
        bail!("local article path escapes its configured directory");
    }
    Ok(path)
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
    image_content_type(source)?;
    Ok(format!("articles/{source}"))
}

fn image_content_type(source: &str) -> Result<&'static str> {
    match source.rsplit_once('.').map(|(_, extension)| extension) {
        Some("png") => Ok("image/png"),
        Some("jpg" | "jpeg") => Ok("image/jpeg"),
        Some("webp") => Ok("image/webp"),
        _ => bail!("unsupported article image format"),
    }
}

fn rasterize_image(source: &str, alt: &str, bytes: &[u8]) -> Result<GithubArticleImage> {
    rasterize_image_with_bounds(source, alt, bytes, IMAGE_WIDTH, IMAGE_PIXEL_HEIGHT)
}

pub(crate) fn rasterize_image_with_bounds(
    source: &str,
    alt: &str,
    bytes: &[u8],
    max_width: u32,
    max_height: u32,
) -> Result<GithubArticleImage> {
    let source_image = image::load_from_memory(bytes).context("unsupported article image")?;
    let (source_width, source_height) = source_image.dimensions();
    if source_width == 0 || source_height == 0 {
        bail!("empty article image");
    }
    let scale = (f64::from(max_width) / f64::from(source_width))
        .min(f64::from(max_height) / f64::from(source_height))
        .min(1.0);
    let width = (f64::from(source_width) * scale).round().max(1.0) as u32;
    let height = (f64::from(source_height) * scale).round().max(1.0) as u32;
    let resized = source_image
        .resize_exact(width, height, FilterType::Triangle)
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

fn markdown_body(markdown: &str) -> &str {
    let mut lines = markdown.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return markdown;
    };
    if first.trim() != "---" {
        return markdown;
    }
    let mut offset = first.len();
    for line in lines {
        offset += line.len();
        if line.trim() == "---" {
            return &markdown[offset..];
        }
    }
    markdown
}

fn frontmatter_labels(markdown: &str) -> Vec<String> {
    let mut lines = markdown.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Vec::new();
    }
    let mut labels = Vec::new();
    let mut reading_labels = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("labels:") {
            reading_labels = true;
            let value = value.trim();
            if let Some(inner) = value
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
            {
                for label in inner.split(',') {
                    push_label(&mut labels, label);
                }
                reading_labels = false;
            } else if !value.is_empty() {
                push_label(&mut labels, value);
                reading_labels = false;
            }
        } else if reading_labels {
            if let Some(label) = trimmed.strip_prefix('-') {
                push_label(&mut labels, label);
            } else if !trimmed.is_empty() {
                reading_labels = false;
            }
        }
    }
    labels
}

fn push_label(labels: &mut Vec<String>, value: &str) {
    let value = value.trim().trim_matches(['\'', '"']);
    if labels.len() >= 6
        || value.is_empty()
        || value.chars().count() > 24
        || value.chars().any(char::is_control)
        || labels
            .iter()
            .any(|label| label.to_lowercase() == value.to_lowercase())
    {
        return;
    }
    labels.push(value.to_owned());
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use svetsec_core::Language;
    use uuid::Uuid;

    use super::{
        GithubSource, article_asset_path, frontmatter_labels, markdown_body, markdown_images,
        markdown_title, rasterize_image, title_from_slug, valid_slug,
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
    fn frontmatter_labels_support_lists_and_case_insensitive_deduplication() {
        assert_eq!(
            frontmatter_labels(
                "---\nlabels:\n  - cryptography\n  - CRYPTOGRAPHY\n  - Rust\n---\n# Title"
            ),
            vec!["cryptography", "Rust"]
        );
        assert_eq!(
            frontmatter_labels("---\nlabels: [cryptography, python]\n---\n# Title"),
            vec!["cryptography", "python"]
        );
        assert_eq!(
            frontmatter_labels("---\nlabels: [Криптография, КРИПТОГРАФИЯ]\n---\n# Title"),
            vec!["Криптография"]
        );
        assert_eq!(
            markdown_body("---\nlabels: [cryptography]\n---\n# Visible title"),
            "# Visible title"
        );
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

    #[tokio::test]
    async fn local_source_reads_current_articles_without_github_cache() {
        let directory = std::env::temp_dir().join(format!("svetsec-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(directory.join("assets"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(directory.join("hello-local"))
            .await
            .unwrap();
        tokio::fs::write(
            directory.join("hello-local/en.md"),
            "---\nlabels: [cryptography]\n---\n# Local title\n\nFirst version\n",
        )
        .await
        .unwrap();
        tokio::fs::write(directory.join("_FORMAT.md"), "# Hidden")
            .await
            .unwrap();
        tokio::fs::write(directory.join("assets/example.png"), [1, 2, 3])
            .await
            .unwrap();

        let source = GithubSource::new(
            "securesvet/svetsec",
            "main".into(),
            None,
            Some(PathBuf::from(&directory)),
        )
        .unwrap();
        let articles = source.list(false, Language::Ru).await.unwrap();
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].slug, "hello-local");
        assert!(articles[0].source_path.ends_with("/en.md"));
        let article = source.article("hello-local", Language::Ru).await.unwrap();
        assert_eq!(article.title, "Local title");
        assert_eq!(article.labels, ["cryptography"]);
        assert_eq!(article.language, "en");
        assert!(!article.markdown.contains("labels:"));
        assert_eq!(
            source
                .asset("assets/example.png")
                .await
                .unwrap()
                .bytes
                .len(),
            3
        );

        tokio::fs::write(directory.join("hello-local/en.md"), "# Updated locally\n")
            .await
            .unwrap();
        assert_eq!(
            source
                .article("hello-local", Language::En)
                .await
                .unwrap()
                .title,
            "Updated locally"
        );

        tokio::fs::write(
            directory.join("hello-local/ru.md"),
            "# Локальный заголовок\n",
        )
        .await
        .unwrap();
        let articles = source.list(false, Language::Ru).await.unwrap();
        assert!(articles[0].source_path.ends_with("/ru.md"));
        let article = source.article("hello-local", Language::Ru).await.unwrap();
        assert_eq!(article.title, "Локальный заголовок");
        assert_eq!(article.language, "ru");
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
}
