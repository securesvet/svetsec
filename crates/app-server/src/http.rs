use std::{net::SocketAddr, sync::Arc};

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tower_http::{services::ServeDir, trace::TraceLayer};
use uuid::Uuid;

use crate::db::{Article, ArticleInput, Database};
use crate::github::{GithubArticle, GithubArticleBody, GithubSource};

const COOKIE_NAME: &str = "svetsec_session";

#[derive(Clone)]
pub struct HttpState {
    db: Database,
    password_hash: Arc<str>,
    secure_cookie: bool,
    github: GithubSource,
}

#[derive(Serialize)]
struct SessionState {
    authenticated: bool,
    owner_online: bool,
    heartbeat_seconds: u8,
}

#[derive(Deserialize)]
struct Login {
    password: String,
}

#[derive(Serialize)]
struct GithubArticleList {
    articles: Vec<GithubArticle>,
    create_url: String,
}

#[derive(Default, Deserialize)]
struct GithubListQuery {
    refresh: Option<u8>,
}

#[derive(Debug)]
struct ApiError(StatusCode, &'static str);

impl HttpState {
    pub fn new(
        db: Database,
        password_hash: String,
        secure_cookie: bool,
        github: GithubSource,
    ) -> Self {
        Self {
            db,
            password_hash: password_hash.into(),
            secure_cookie,
            github,
        }
    }
}

pub async fn serve(
    address: SocketAddr,
    state: HttpState,
    static_dir: String,
) -> std::io::Result<()> {
    let app = Router::new()
        .route("/api/session", get(session).post(login).delete(logout))
        .route("/api/heartbeat", post(heartbeat))
        .route("/api/articles", get(articles).post(save_article))
        .route("/api/github/articles", get(github_articles))
        .route("/api/github/articles/{slug}", get(github_article))
        .route("/api/github/assets/{*path}", get(github_asset))
        .fallback_service(ServeDir::new(static_dir).append_index_html_on_directories(true))
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
    state_response(&state, token_from(&headers).as_deref(), false)
}

async fn heartbeat(
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

    let mut response = Json(SessionState {
        authenticated: true,
        owner_online: true,
        heartbeat_seconds: 15,
    })
    .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&session_cookie(&token, state.secure_cookie))
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "invalid session cookie"))?,
    );
    Ok(response)
}

async fn logout(State(state): State<HttpState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = token_from(&headers) {
        state.db.delete_web_session(&token).map_err(internal)?;
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

async fn github_articles(
    State(state): State<HttpState>,
    Query(query): Query<GithubListQuery>,
) -> Result<Json<GithubArticleList>, ApiError> {
    let articles = state
        .github
        .list(query.refresh == Some(1))
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
) -> Result<Json<GithubArticleBody>, ApiError> {
    state
        .github
        .article(&slug)
        .await
        .map(Json)
        .map_err(github_error)
}

async fn github_asset(
    State(state): State<HttpState>,
    Path(path): Path<String>,
) -> Result<Response, ApiError> {
    let asset = state.github.asset(&path).await.map_err(github_error)?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(asset.content_type),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=300"),
            ),
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
    let authenticated = match token {
        Some(token) if touch => state.db.touch_web_session(token).map_err(internal)?,
        Some(token) => state.db.is_web_session(token).map_err(internal)?,
        None => false,
    };
    Ok(Json(SessionState {
        authenticated,
        owner_online: state.db.owner_online().map_err(internal)?,
        heartbeat_seconds: 15,
    }))
}

fn authenticated(state: &HttpState, headers: &HeaderMap) -> Result<bool, ApiError> {
    token_from(headers)
        .map(|token| state.db.is_web_session(&token).map_err(internal))
        .unwrap_or(Ok(false))
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

fn expired_cookie(secure: bool) -> String {
    format!(
        "{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
        if secure { "; Secure" } else { "" }
    )
}

fn validate_article(article: &ArticleInput) -> Result<(), ApiError> {
    let valid_slug = !article.slug.is_empty()
        && article.slug.len() <= 80
        && article
            .slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !valid_slug {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid slug"));
    }
    if article.title_en.trim().is_empty()
        || article.title_ru.trim().is_empty()
        || article.body_en.len() > 200_000
        || article.body_ru.len() > 200_000
    {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid article"));
    }
    Ok(())
}

fn internal(error: rusqlite::Error) -> ApiError {
    tracing::error!(%error, "database request failed");
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, "database error")
}

fn github_error(error: anyhow::Error) -> ApiError {
    tracing::warn!(%error, "GitHub article request failed");
    ApiError(
        StatusCode::BAD_GATEWAY,
        "GitHub articles are temporarily unavailable",
    )
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}
