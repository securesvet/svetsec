mod db;
mod github;
mod http;
mod python;
mod ssh;

use std::{env, error::Error, net::SocketAddr, path::PathBuf};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use db::Database;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = dotenvy::dotenv();
    if env::args().nth(1).as_deref() == Some("hash-password") {
        let password = env::args()
            .nth(2)
            .ok_or("usage: svetsec-server hash-password <password>")?;
        let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes())
            .map_err(|error| format!("could not create password salt: {error}"))?;
        println!(
            "{}",
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map_err(|error| format!("could not hash password: {error}"))?
        );
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "svetsec_server=info,tower_http=info".into()),
        )
        .init();

    let database = Database::open(env_or("SVETSEC_DATABASE", "svetsec.db"))?;
    let password_hash = env::var("SVETSEC_OWNER_PASSWORD_HASH").map_err(|_| {
        "SVETSEC_OWNER_PASSWORD_HASH is required; generate it with `cargo run -p svetsec-server -- hash-password <password>`"
    })?;
    let address: SocketAddr = env_or("SVETSEC_HTTP_ADDR", "127.0.0.1:3000").parse()?;
    let static_dir = env_or("SVETSEC_STATIC_DIR", "dist");
    let secure_cookie = env_or("SVETSEC_SECURE_COOKIE", "true") != "false";
    let articles_dir = match env_or("SVETSEC_ARTICLES_SOURCE", "local").as_str() {
        "github" => None,
        "local" => Some(PathBuf::from(env_or("SVETSEC_ARTICLES_DIR", "articles"))),
        _ => return Err("SVETSEC_ARTICLES_SOURCE must be `github` or `local`".into()),
    };
    let github = github::GithubSource::new(
        &env_or("SVETSEC_GITHUB_REPOSITORY", "securesvet/svetsec"),
        env_or("SVETSEC_GITHUB_BRANCH", "main"),
        env::var("SVETSEC_GITHUB_TOKEN").ok(),
        articles_dir,
    )?;
    if github.is_local() {
        tracing::info!("using local articles directory");
    }
    let pyodide = python::PyodideRunner::from_env();
    let http_github = github.clone();
    let http_pyodide = pyodide.clone();
    let http_database = database.clone();
    let http_server = async move {
        http::serve(
            address,
            http::HttpState::new(
                http_database,
                password_hash,
                secure_cookie,
                http_github,
                http_pyodide,
            ),
            static_dir,
        )
        .await
        .map_err(anyhow::Error::from)
    };

    let owner_key_file = env::var("SVETSEC_OWNER_PUBLIC_KEY_FILE").ok();
    let host_key_file = env::var("SVETSEC_SSH_HOST_KEY_FILE").ok();
    if let (Some(owner_key_file), Some(host_key_file)) = (owner_key_file, host_key_file) {
        let owner_key_text = std::fs::read_to_string(owner_key_file)?;
        let owner_key = russh::keys::ssh_key::PublicKey::from_openssh(&owner_key_text)?;
        let host_key = russh::keys::load_secret_key(host_key_file, None)?;
        let ssh_address: SocketAddr = env_or("SVETSEC_SSH_ADDR", "0.0.0.0:2222").parse()?;
        let owner_user = env_or("SVETSEC_OWNER_USER", "owner");
        tokio::try_join!(
            http_server,
            ssh::serve(
                ssh_address,
                database,
                owner_user,
                owner_key,
                host_key,
                github,
                pyodide
            )
        )?;
    } else {
        tracing::warn!(
            "SSH disabled: set SVETSEC_OWNER_PUBLIC_KEY_FILE and SVETSEC_SSH_HOST_KEY_FILE"
        );
        http_server.await?;
    }
    Ok(())
}

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}
