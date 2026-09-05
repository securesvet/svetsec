use std::{net::SocketAddr, sync::Arc};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
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
use svetsec_core::{Language, markdown_code_blocks};

const COOKIE_NAME: &str = "svetsec_session";

#[derive(Clone)]
pub struct HttpState {
    db: Database,
    password_hash: Arc<str>,
    secure_cookie: bool,
    github: GithubSource,
    pyodide: PyodideRunner,
}

#[derive(Serialize)]
struct SessionState {
    authenticated: bool,
    username: Option<String>,
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
    ) -> Self {
        Self {
            db,
            password_hash: password_hash.into(),
            secure_cookie,
            github,
            pyodide,
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
        .route("/api/articles", get(articles).post(save_article))
        .route(
            "/api/articles/{slug}/comments",
            get(comments).post(add_comment),
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
    session_with_cookie(
        SessionState {
            authenticated: false,
            username: Some(user.username),
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
    session_with_cookie(
        SessionState {
            authenticated: false,
            username: Some(credentials.user.username),
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
    Ok(Json(SessionState {
        authenticated: identity.owner,
        username: identity.user.map(|user| user.username),
    }))
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
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| {
            cookie
                .strip_prefix(&format!("{COOKIE_NAME}="))
                .map(str::to_owned)
        })
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
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && !matches!(
            username.to_ascii_lowercase().as_str(),
            "guest" | "owner" | "svetsec"
        );
    let password_length = input.password.chars().count();
    if !username_valid || !(8..=128).contains(&password_length) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "username or password does not meet requirements",
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

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        UserLogin, hash_password, session_cookie, validate_comment, validate_user,
        verify_user_password,
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
        assert!(
            validate_user(&UserLogin {
                username: "guest".into(),
                password: "correct horse battery staple".into(),
            })
            .is_err()
        );
        assert!(
            validate_user(&UserLogin {
                username: "bad name".into(),
                password: "password".into(),
            })
            .is_err()
        );
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
    }
}
