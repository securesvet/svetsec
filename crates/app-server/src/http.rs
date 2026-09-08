use std::{io::Cursor, net::SocketAddr, sync::Arc};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use uuid::Uuid;

use crate::db::{
    Article, ArticleInput, Comment, CommentAuthor, CreateCommentError, Database, User,
};
use crate::github::{GithubArticle, GithubArticleBody, GithubSource};
use crate::python::PyodideRunner;
use crate::telegram::{TelegramAuth, pkce_challenge};
use svetsec_core::{Language, markdown_code_blocks};

const COOKIE_NAME: &str = "svetsec_session";
const TELEGRAM_STATE_COOKIE: &str = "svetsec_telegram_state";
const MAX_AVATAR_BYTES: usize = 3 * 1024 * 1024;

#[derive(Clone)]
pub struct HttpState {
    db: Database,
    password_hash: Arc<str>,
    secure_cookie: bool,
    github: GithubSource,
    pyodide: PyodideRunner,
    telegram: Option<TelegramAuth>,
}

#[derive(Serialize)]
struct SessionState {
    authenticated: bool,
    username: Option<String>,
    avatar_url: Option<String>,
    telegram_enabled: bool,
    can_moderate_comments: bool,
}

#[derive(Deserialize)]
struct Login {
    password: String,
}

#[derive(Deserialize)]
struct UserLogin {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct CommentInput {
    body: String,
}

struct Identity {
    owner: bool,
    user: Option<User>,
}

#[derive(Serialize)]
struct GithubArticleList {
    articles: Vec<GithubArticle>,
    create_url: String,
}

#[derive(Serialize)]
struct PythonOutput {
    output: String,
}

#[derive(Default, Deserialize)]
struct GithubListQuery {
    refresh: Option<u8>,
    lang: Option<String>,
}

#[derive(Default, Deserialize)]
struct LanguageQuery {
    lang: Option<String>,
}

#[derive(Default, Deserialize)]
struct TelegramStartQuery {
    next: Option<String>,
}

#[derive(Default, Deserialize)]
struct TelegramCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

impl LanguageQuery {
    fn language(&self) -> Language {
        self.lang
            .as_deref()
            .and_then(Language::from_code)
            .unwrap_or_default()
    }
}

impl GithubListQuery {
    fn language(&self) -> Language {
        self.lang
            .as_deref()
            .and_then(Language::from_code)
            .unwrap_or_default()
    }
}

#[derive(Debug)]
struct ApiError(StatusCode, &'static str);

impl HttpState {
    pub fn new(
        db: Database,
        password_hash: String,
        secure_cookie: bool,
        github: GithubSource,
        pyodide: PyodideRunner,
        telegram: Option<TelegramAuth>,
    ) -> Self {
        Self {
            db,
            password_hash: password_hash.into(),
            secure_cookie,
            github,
            pyodide,
            telegram,
        }
    }
}

pub async fn serve(
    address: SocketAddr,
    state: HttpState,
    static_dir: String,
) -> std::io::Result<()> {
    let index = std::path::Path::new(&static_dir).join("index.html");
    let resume = std::path::Path::new(&static_dir)
        .join("assets")
        .join("resume.pdf");
    let static_files = ServeDir::new(static_dir)
        .append_index_html_on_directories(true)
        .fallback(ServeFile::new(index));
    let app = Router::new()
        .route("/api/session", get(session).post(login).delete(logout))
        .route("/api/users", post(register))
        .route("/api/users/session", post(user_login))
        .route("/api/users/avatar", post(upload_avatar))
        .route("/api/users/{user_id}/avatar", get(user_avatar))
        .route("/api/auth/telegram/start", get(telegram_start))
        .route("/api/auth/telegram/callback", get(telegram_callback))
        .route("/api/articles", get(articles).post(save_article))
        .route(
            "/api/articles/{slug}/comments",
            get(comments).post(add_comment),
        )
        .route(
            "/api/articles/{slug}/comments/{comment_id}",
            delete(remove_comment),
        )
        .route("/api/github/articles", get(github_articles))
        .route("/api/github/articles/{slug}", get(github_article))
        .route(
            "/api/github/articles/{slug}/python/{block}",
            post(run_github_python),
        )
        .route("/api/github/assets/{*path}", get(github_asset))
        .route_service("/resume", ServeFile::new(resume))
        .fallback_service(static_files)
        .layer(DefaultBodyLimit::max(MAX_AVATAR_BYTES))
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "HTTP server listening");
    axum::serve(listener, app).await
}

async fn session(
    State(state): State<HttpState>,
    headers: HeaderMap,
) -> Result<Json<SessionState>, ApiError> {
    state_response(&state, token_from(&headers).as_deref(), true)
}

async fn login(
    State(state): State<HttpState>,
    Json(input): Json<Login>,
) -> Result<Response, ApiError> {
    let hash = PasswordHash::new(&state.password_hash).map_err(|_| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "owner login is not configured",
        )
    })?;
    Argon2::default()
        .verify_password(input.password.as_bytes(), &hash)
        .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "invalid credentials"))?;

    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    state.db.create_web_session(&token).map_err(internal)?;
    state.db.touch_web_session(&token).map_err(internal)?;

    session_with_cookie(
        SessionState {
            authenticated: true,
            username: None,
            avatar_url: Some("/assets/profile.jpg".into()),
            telegram_enabled: state.telegram.is_some(),
            can_moderate_comments: true,
        },
        &token,
        state.secure_cookie,
        StatusCode::OK,
    )
}

async fn register(
    State(state): State<HttpState>,
    Json(input): Json<UserLogin>,
) -> Result<Response, ApiError> {
    validate_user(&input)?;
    let password_hash = hash_password(input.password).await?;
    let user = state
        .db
        .create_user(input.username.trim(), &password_hash)
        .map_err(registration_error)?;
    let token = new_session_token();
    state
        .db
        .create_user_session(&token, user.id)
        .map_err(internal)?;
    let avatar_url = user.avatar_url();
    session_with_cookie(
        SessionState {
            authenticated: false,
            username: Some(user.username),
            avatar_url,
            telegram_enabled: state.telegram.is_some(),
            can_moderate_comments: false,
        },
        &token,
        state.secure_cookie,
        StatusCode::CREATED,
    )
}

async fn user_login(
    State(state): State<HttpState>,
    Json(input): Json<UserLogin>,
) -> Result<Response, ApiError> {
    validate_user(&input)?;
    let credentials = state
        .db
        .user_credentials(input.username.trim())
        .map_err(internal)?
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid credentials"))?;
    verify_user_password(input.password, credentials.password_hash).await?;
    let token = new_session_token();
    state
        .db
        .create_user_session(&token, credentials.user.id)
        .map_err(internal)?;
    let can_moderate_comments = state
        .db
        .user_can_moderate_comments(credentials.user.id)
        .map_err(internal)?;
    let avatar_url = credentials.user.avatar_url();
    session_with_cookie(
        SessionState {
            authenticated: false,
            username: Some(credentials.user.username),
            avatar_url,
            telegram_enabled: state.telegram.is_some(),
            can_moderate_comments,
        },
        &token,
        state.secure_cookie,
        StatusCode::OK,
    )
}

async fn logout(State(state): State<HttpState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = token_from(&headers) {
        state.db.delete_web_session(&token).map_err(internal)?;
        state.db.delete_user_session(&token).map_err(internal)?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&expired_cookie(state.secure_cookie))
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "invalid session cookie"))?,
    );
    Ok(response)
}

async fn telegram_start(
    State(state): State<HttpState>,
    Query(query): Query<TelegramStartQuery>,
) -> Result<Response, ApiError> {
    let telegram = state.telegram.as_ref().ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "Telegram login is not configured",
    ))?;
    let oauth_state = new_session_token();
    let nonce = new_session_token();
    let verifier = new_session_token();
    let next_path = safe_next_path(query.next.as_deref());
    state
        .db
        .create_telegram_login_attempt(&oauth_state, &verifier, &nonce, &next_path)
        .map_err(internal)?;
    let location = telegram.authorization_url(&oauth_state, &nonce, &pkce_challenge(&verifier));
    let mut response = Redirect::temporary(&location).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&telegram_state_cookie(
            &oauth_state,
            state.secure_cookie,
            600,
        ))
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "invalid login cookie"))?,
    );
    Ok(response)
}

async fn telegram_callback(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Query(query): Query<TelegramCallbackQuery>,
) -> Result<Response, ApiError> {
    if query.error.is_some() {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "Telegram login was cancelled",
        ));
    }
    let returned_state = query
        .state
        .as_deref()
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "missing OAuth state"))?;
    let cookie_state = cookie_from(&headers, TELEGRAM_STATE_COOKIE).ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "missing OAuth state cookie",
    ))?;
    if cookie_state != returned_state {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid OAuth state"));
    }
    let code = query.code.ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "missing authorization code",
    ))?;
    let attempt = state
        .db
        .consume_telegram_login_attempt(returned_state)
        .map_err(internal)?
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "expired OAuth state"))?;
    let telegram = state.telegram.as_ref().ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "Telegram login is not configured",
    ))?;
    let profile = telegram
        .exchange(code, attempt.code_verifier, attempt.nonce)
        .await
        .map_err(telegram_error)?;
    let claim_comment_moderator = profile
        .verified_username
        .as_deref()
        .is_some_and(|username| {
            username
                .trim_start_matches('@')
                .eq_ignore_ascii_case("svetsec")
        });
    let user = state
        .db
        .upsert_telegram_user(
            &profile.id,
            profile.username.as_deref(),
            profile.picture.as_deref(),
            claim_comment_moderator,
        )
        .map_err(registration_error)?;
    let token = new_session_token();
    state
        .db
        .create_user_session(&token, user.id)
        .map_err(internal)?;
    let mut response = Redirect::to(&attempt.next_path).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&session_cookie(&token, state.secure_cookie))
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "invalid session cookie"))?,
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&telegram_state_cookie("", state.secure_cookie, 0))
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "invalid login cookie"))?,
    );
    Ok(response)
}

async fn upload_avatar(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<SessionState>, ApiError> {
    let identity = identity(&state, token_from(&headers).as_deref(), true)?;
    let user = identity.user.ok_or(ApiError(
        StatusCode::UNAUTHORIZED,
        "reader session required",
    ))?;
    if body.is_empty() || body.len() > MAX_AVATAR_BYTES {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "avatar is too large",
        ));
    }
    let encoded = tokio::task::spawn_blocking(move || normalize_avatar(&body))
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "avatar processing failed",
            )
        })??;
    let user = state
        .db
        .set_user_avatar(user.id, &encoded, "image/jpeg")
        .map_err(internal)?;
    let can_moderate_comments = state
        .db
        .user_can_moderate_comments(user.id)
        .map_err(internal)?;
    let avatar_url = user.avatar_url();
    Ok(Json(SessionState {
        authenticated: false,
        username: Some(user.username),
        avatar_url,
        telegram_enabled: state.telegram.is_some(),
        can_moderate_comments,
    }))
}

async fn user_avatar(
    State(state): State<HttpState>,
    Path(user_id): Path<i64>,
) -> Result<Response, ApiError> {
    let avatar = state
        .db
        .user_avatar(user_id)
        .map_err(internal)?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "avatar not found"))?;
    let content_type = HeaderValue::from_str(&avatar.content_type)
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "invalid avatar type"))?;
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
        ],
        avatar.bytes,
    )
        .into_response())
}

async fn articles(
    State(state): State<HttpState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Article>>, ApiError> {
    let authenticated = authenticated(&state, &headers)?;
    state
        .db
        .list_articles(authenticated)
        .map(Json)
        .map_err(internal)
}

async fn save_article(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(input): Json<ArticleInput>,
) -> Result<(StatusCode, Json<Article>), ApiError> {
    if !authenticated(&state, &headers)? {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "owner session required"));
    }
    validate_article(&input)?;
    state
        .db
        .save_article(&input)
        .map(|article| (StatusCode::CREATED, Json(article)))
        .map_err(internal)
}

async fn comments(
    State(state): State<HttpState>,
    Path(slug): Path<String>,
) -> Result<Json<Vec<Comment>>, ApiError> {
    validate_slug(&slug)?;
    state.db.list_comments(&slug).map(Json).map_err(internal)
}

async fn add_comment(
    State(state): State<HttpState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(input): Json<CommentInput>,
) -> Result<(StatusCode, Json<Comment>), ApiError> {
    validate_slug(&slug)?;
    let body = validate_comment(&input.body)?;
    let identity = identity(&state, token_from(&headers).as_deref(), true)?;
    let author = if identity.owner {
        CommentAuthor::Owner
    } else if let Some(user) = identity.user {
        CommentAuthor::User(user.id)
    } else {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "registration or login required",
        ));
    };
    let comment = state
        .db
        .create_comment(&slug, author, body)
        .map_err(comment_error)?;
    Ok((StatusCode::CREATED, Json(comment)))
}

async fn remove_comment(
    State(state): State<HttpState>,
    Path((slug, comment_id)): Path<(String, i64)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    validate_slug(&slug)?;
    if comment_id < 1 {
        return Err(ApiError(StatusCode::NOT_FOUND, "comment not found"));
    }
    let identity = identity(&state, token_from(&headers).as_deref(), true)?;
    if !identity.owner && identity.user.is_none() {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "login required"));
    }
    if !identity_can_moderate_comments(&state, &identity)? {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "comment moderator session required",
        ));
    }
    if !state
        .db
        .delete_comment(&slug, comment_id)
        .map_err(internal)?
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "comment not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn github_articles(
    State(state): State<HttpState>,
    Query(query): Query<GithubListQuery>,
) -> Result<Json<GithubArticleList>, ApiError> {
    let articles = state
        .github
        .list(query.refresh == Some(1), query.language())
        .await
        .map_err(github_error)?;
    Ok(Json(GithubArticleList {
        articles,
        create_url: state.github.editor_url(None),
    }))
}

async fn github_article(
    State(state): State<HttpState>,
    Path(slug): Path<String>,
    Query(query): Query<LanguageQuery>,
) -> Result<Json<GithubArticleBody>, ApiError> {
    state
        .github
        .article(&slug, query.language())
        .await
        .map(Json)
        .map_err(github_error)
}

async fn run_github_python(
    State(state): State<HttpState>,
    Path((slug, block_index)): Path<(String, usize)>,
    Query(query): Query<LanguageQuery>,
) -> Result<Json<PythonOutput>, ApiError> {
    let article = state
        .github
        .article(&slug, query.language())
        .await
        .map_err(github_error)?;
    let block = markdown_code_blocks(&article.markdown)
        .into_iter()
        .find(|block| block.index == block_index)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "code block not found"))?;
    if !block.executable() || block.code.trim().is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "code block is not executable Python",
        ));
    }
    let output = state.pyodide.run(&block.code).await.map_err(python_error)?;
    Ok(Json(PythonOutput { output }))
}

async fn github_asset(
    State(state): State<HttpState>,
    Path(path): Path<String>,
) -> Result<Response, ApiError> {
    let asset = state.github.asset(&path).await.map_err(github_error)?;
    let cache_control = if state.github.is_local() {
        HeaderValue::from_static("no-store")
    } else {
        HeaderValue::from_static("public, max-age=300")
    };
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(asset.content_type),
            ),
            (header::CACHE_CONTROL, cache_control),
        ],
        asset.bytes,
    )
        .into_response())
}

fn state_response(
    state: &HttpState,
    token: Option<&str>,
    touch: bool,
) -> Result<Json<SessionState>, ApiError> {
    let identity = identity(state, token, touch)?;
    let can_moderate_comments = identity_can_moderate_comments(state, &identity)?;
    Ok(Json(SessionState {
        authenticated: identity.owner,
        username: identity.user.as_ref().map(|user| user.username.clone()),
        avatar_url: if identity.owner {
            Some("/assets/profile.jpg".into())
        } else {
            identity.user.and_then(|user| user.avatar_url())
        },
        telegram_enabled: state.telegram.is_some(),
        can_moderate_comments,
    }))
}

fn identity_can_moderate_comments(
    state: &HttpState,
    identity: &Identity,
) -> Result<bool, ApiError> {
    if identity.owner {
        return Ok(true);
    }
    identity
        .user
        .as_ref()
        .map(|user| state.db.user_can_moderate_comments(user.id))
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(internal)
}

fn authenticated(state: &HttpState, headers: &HeaderMap) -> Result<bool, ApiError> {
    token_from(headers)
        .map(|token| state.db.is_web_session(&token).map_err(internal))
        .unwrap_or(Ok(false))
}

fn identity(state: &HttpState, token: Option<&str>, touch: bool) -> Result<Identity, ApiError> {
    let Some(token) = token else {
        return Ok(Identity {
            owner: false,
            user: None,
        });
    };
    let owner = if touch {
        state.db.touch_web_session(token).map_err(internal)?
    } else {
        state.db.is_web_session(token).map_err(internal)?
    };
    let user = if owner {
        None
    } else {
        state.db.user_for_session(token, touch).map_err(internal)?
    };
    Ok(Identity { owner, user })
}

fn token_from(headers: &HeaderMap) -> Option<String> {
    cookie_from(headers, COOKIE_NAME)
}

fn cookie_from(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix(&format!("{name}=")).map(str::to_owned))
}

fn telegram_state_cookie(state: &str, secure: bool, max_age: u16) -> String {
    format!(
        "{TELEGRAM_STATE_COOKIE}={state}; Path=/api/auth/telegram/callback; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    )
}

fn safe_next_path(path: Option<&str>) -> String {
    path.filter(|path| {
        path.starts_with('/')
            && !path.starts_with("//")
            && path.len() <= 200
            && !path.chars().any(char::is_control)
    })
    .unwrap_or("/")
    .to_owned()
}

fn normalize_avatar(bytes: &[u8]) -> Result<Vec<u8>, ApiError> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid avatar image"))?;
    let format = reader
        .format()
        .filter(|format| {
            matches!(
                format,
                image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::WebP
            )
        })
        .ok_or(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported avatar image",
        ))?;
    let (width, height) = image::ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid avatar image"))?;
    if width > 4096 || height > 4096 {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "avatar dimensions are too large",
        ));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid avatar image"))?;
    let thumbnail = if width > 512 || height > 512 {
        image.thumbnail(512, 512)
    } else {
        image
    }
    .to_rgb8();
    let mut output = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(thumbnail)
        .write_to(&mut output, image::ImageFormat::Jpeg)
        .map_err(|_| ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid avatar image"))?;
    Ok(output.into_inner())
}

fn session_cookie(token: &str, secure: bool) -> String {
    format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000{}",
        if secure { "; Secure" } else { "" }
    )
}

fn session_with_cookie(
    state: SessionState,
    token: &str,
    secure: bool,
    status: StatusCode,
) -> Result<Response, ApiError> {
    let mut response = (status, Json(state)).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&session_cookie(token, secure))
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "invalid session cookie"))?,
    );
    Ok(response)
}

fn new_session_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn expired_cookie(secure: bool) -> String {
    format!(
        "{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
        if secure { "; Secure" } else { "" }
    )
}

fn validate_article(article: &ArticleInput) -> Result<(), ApiError> {
    validate_slug(&article.slug)?;
    if article.title_en.trim().is_empty()
        || article.title_ru.trim().is_empty()
        || article.body_en.len() > 200_000
        || article.body_ru.len() > 200_000
    {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid article"));
    }
    Ok(())
}

fn validate_slug(slug: &str) -> Result<(), ApiError> {
    let valid = !slug.is_empty()
        && slug.len() <= 80
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    valid
        .then_some(())
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "invalid slug"))
}

fn validate_user(input: &UserLogin) -> Result<(), ApiError> {
    let username = input.username.trim();
    let username_valid = (3..=24).contains(&username.len())
        && username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if !username_valid {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "username must use 3-24 Latin letters, numbers, _ or -",
        ));
    }
    if matches!(
        username.to_ascii_lowercase().as_str(),
        "guest" | "owner" | "svetsec"
    ) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "username is reserved"));
    }
    let password_length = input.password.chars().count();
    if !(8..=128).contains(&password_length) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "password must contain 8-128 characters",
        ));
    }
    Ok(())
}

fn validate_comment(body: &str) -> Result<&str, ApiError> {
    let body = body.trim();
    let valid = !body.is_empty()
        && body.chars().count() <= 1_000
        && body
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\n' | '\t'));
    valid
        .then_some(body)
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "invalid comment"))
}

async fn hash_password(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes()).map_err(|_| ())?;
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| ())
    })
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "password hashing failed"))?
    .map_err(|()| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "password hashing failed"))
}

async fn verify_user_password(password: String, hash: String) -> Result<(), ApiError> {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash)
            .ok()
            .and_then(|hash| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &hash)
                    .ok()
            })
            .ok_or(())
    })
    .await
    .map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "password verification failed",
        )
    })?
    .map_err(|()| ApiError(StatusCode::UNAUTHORIZED, "invalid credentials"))
}

fn internal(error: rusqlite::Error) -> ApiError {
    tracing::error!(%error, "database request failed");
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, "database error")
}

fn registration_error(error: rusqlite::Error) -> ApiError {
    if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
        ApiError(StatusCode::CONFLICT, "username is already registered")
    } else {
        internal(error)
    }
}

fn comment_error(error: CreateCommentError) -> ApiError {
    match error {
        CreateCommentError::RateLimited => ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "please wait before commenting again",
        ),
        CreateCommentError::Database(error) => internal(error),
    }
}

fn github_error(error: anyhow::Error) -> ApiError {
    tracing::warn!(%error, "article source request failed");
    ApiError(
        StatusCode::BAD_GATEWAY,
        "articles are temporarily unavailable",
    )
}

fn python_error(error: anyhow::Error) -> ApiError {
    tracing::warn!(%error, "Pyodide execution failed");
    ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Python execution is temporarily unavailable",
    )
}

fn telegram_error(error: anyhow::Error) -> ApiError {
    tracing::warn!(%error, "Telegram login failed");
    ApiError(StatusCode::BAD_GATEWAY, "Telegram login failed")
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        UserLogin, hash_password, normalize_avatar, safe_next_path, session_cookie,
        telegram_state_cookie, validate_comment, validate_user, verify_user_password,
    };

    #[test]
    fn public_account_and_comment_inputs_are_bounded() {
        assert!(
            validate_user(&UserLogin {
                username: "Reader_1".into(),
                password: "correct horse battery staple".into(),
            })
            .is_ok()
        );
        let reserved = validate_user(&UserLogin {
            username: "guest".into(),
            password: "correct horse battery staple".into(),
        })
        .unwrap_err();
        assert_eq!(reserved.1, "username is reserved");
        let invalid_name = validate_user(&UserLogin {
            username: "bad name".into(),
            password: "password".into(),
        })
        .unwrap_err();
        assert_eq!(
            invalid_name.1,
            "username must use 3-24 Latin letters, numbers, _ or -"
        );
        let invalid_password = validate_user(&UserLogin {
            username: "reader".into(),
            password: "short".into(),
        })
        .unwrap_err();
        assert_eq!(invalid_password.1, "password must contain 8-128 characters");
        assert!(validate_comment("A useful comment").is_ok());
        assert!(validate_comment("\u{1b}[31mterminal escape").is_err());
        assert!(validate_comment(&"x".repeat(1_001)).is_err());
    }

    #[tokio::test]
    async fn reader_passwords_use_verifiable_argon_hashes() {
        let hash = hash_password("correct horse battery staple".into())
            .await
            .expect("password hash");
        assert!(hash.starts_with("$argon2id$"));
        verify_user_password("correct horse battery staple".into(), hash.clone())
            .await
            .expect("valid password");
        assert!(
            verify_user_password("not the password".into(), hash)
                .await
                .is_err()
        );
    }

    #[test]
    fn production_session_cookie_is_host_only_and_script_inaccessible() {
        let cookie = session_cookie("token", true);
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Secure"));
        assert!(!cookie.contains("Domain="));

        let state_cookie = telegram_state_cookie("state", true, 600);
        assert!(state_cookie.contains("HttpOnly"));
        assert!(state_cookie.contains("SameSite=Lax"));
        assert!(state_cookie.contains("Path=/api/auth/telegram/callback"));
    }

    #[test]
    fn telegram_return_paths_cannot_leave_this_site() {
        assert_eq!(safe_next_path(Some("/articles/hello")), "/articles/hello");
        assert_eq!(safe_next_path(Some("//evil.example")), "/");
        assert_eq!(safe_next_path(Some("https://evil.example")), "/");
        assert_eq!(safe_next_path(Some("/bad\nheader")), "/");
    }

    #[test]
    fn uploaded_avatars_are_normalized_and_dimension_limited() {
        let mut source = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(32, 16)
            .write_to(&mut source, image::ImageFormat::Png)
            .expect("source PNG");
        let normalized = normalize_avatar(source.get_ref()).expect("normalized avatar");
        let avatar = image::load_from_memory(&normalized).expect("normalized JPEG");
        assert_eq!((avatar.width(), avatar.height()), (32, 16));

        let mut oversized = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(4097, 1)
            .write_to(&mut oversized, image::ImageFormat::Png)
            .expect("oversized PNG");
        let error = normalize_avatar(oversized.get_ref()).expect_err("dimension limit");
        assert_eq!(error.0, axum::http::StatusCode::PAYLOAD_TOO_LARGE);
    }
}
