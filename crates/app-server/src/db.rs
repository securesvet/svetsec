use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PRESENCE_TTL_SECONDS: i64 = 45;
const SESSION_TTL_SECONDS: i64 = 60 * 60 * 24 * 30;

#[derive(Clone)]
pub struct Database(Arc<Mutex<Connection>>);

#[derive(Debug, Clone, Serialize)]
pub struct Article {
    pub id: i64,
    pub slug: String,
    pub title_en: String,
    pub title_ru: String,
    pub body_en: String,
    pub body_ru: String,
    pub published: bool,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct ArticleInput {
    pub slug: String,
    pub title_en: String,
    pub title_ru: String,
    pub body_en: String,
    pub body_ru: String,
    #[serde(default)]
    pub published: bool,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS web_sessions (
                token_hash TEXT PRIMARY KEY,
                created_at INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS presence (
                id TEXT PRIMARY KEY,
                transport TEXT NOT NULL CHECK (transport IN ('web', 'ssh')),
                last_seen INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS articles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                slug TEXT NOT NULL UNIQUE,
                title_en TEXT NOT NULL,
                title_ru TEXT NOT NULL,
                body_en TEXT NOT NULL,
                body_ru TEXT NOT NULL,
                published INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_presence_last_seen ON presence(last_seen);
            CREATE INDEX IF NOT EXISTS idx_articles_updated_at ON articles(updated_at DESC);
            ",
        )?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    pub fn create_web_session(&self, token: &str) -> rusqlite::Result<()> {
        let now = now();
        self.connection().execute(
            "INSERT INTO web_sessions(token_hash, created_at, last_seen, expires_at)
             VALUES (?1, ?2, ?2, ?3)",
            params![hash_token(token), now, now + SESSION_TTL_SECONDS],
        )?;
        Ok(())
    }

    pub fn touch_web_session(&self, token: &str) -> rusqlite::Result<bool> {
        let now = now();
        let token_hash = hash_token(token);
        let changed = self.connection().execute(
            "UPDATE web_sessions SET last_seen = ?1
             WHERE token_hash = ?2 AND expires_at > ?1",
            params![now, token_hash],
        )?;
        if changed == 1 {
            self.touch_presence(&format!("web:{token_hash}"), "web")?;
        }
        Ok(changed == 1)
    }

    pub fn is_web_session(&self, token: &str) -> rusqlite::Result<bool> {
        let now = now();
        self.connection()
            .query_row(
                "SELECT 1 FROM web_sessions WHERE token_hash = ?1 AND expires_at > ?2",
                params![hash_token(token), now],
                |_| Ok(true),
            )
            .optional()
            .map(|value| value.unwrap_or(false))
    }

    pub fn delete_web_session(&self, token: &str) -> rusqlite::Result<()> {
        let token_hash = hash_token(token);
        let connection = self.connection();
        connection.execute(
            "DELETE FROM web_sessions WHERE token_hash = ?1",
            [&token_hash],
        )?;
        connection.execute(
            "DELETE FROM presence WHERE id = ?1",
            [format!("web:{token_hash}")],
        )?;
        Ok(())
    }

    pub fn touch_presence(&self, id: &str, transport: &str) -> rusqlite::Result<()> {
        self.connection().execute(
            "INSERT INTO presence(id, transport, last_seen) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET transport = excluded.transport,
             last_seen = excluded.last_seen",
            params![id, transport, now()],
        )?;
        Ok(())
    }

    pub fn remove_presence(&self, id: &str) -> rusqlite::Result<()> {
        self.connection()
            .execute("DELETE FROM presence WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn owner_online(&self) -> rusqlite::Result<bool> {
        let threshold = now() - PRESENCE_TTL_SECONDS;
        self.cleanup()?;
        self.connection()
            .query_row(
                "SELECT 1 FROM presence WHERE last_seen >= ?1 LIMIT 1",
                [threshold],
                |_| Ok(true),
            )
            .optional()
            .map(|value| value.unwrap_or(false))
    }

    pub fn list_articles(&self, include_drafts: bool) -> rusqlite::Result<Vec<Article>> {
        let connection = self.connection();
        let sql = if include_drafts {
            "SELECT id, slug, title_en, title_ru, body_en, body_ru, published, updated_at
             FROM articles ORDER BY updated_at DESC"
        } else {
            "SELECT id, slug, title_en, title_ru, body_en, body_ru, published, updated_at
             FROM articles WHERE published = 1 ORDER BY updated_at DESC"
        };
        let mut statement = connection.prepare(sql)?;
        statement
            .query_map([], |row| {
                Ok(Article {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    title_en: row.get(2)?,
                    title_ru: row.get(3)?,
                    body_en: row.get(4)?,
                    body_ru: row.get(5)?,
                    published: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect()
    }

    pub fn save_article(&self, article: &ArticleInput) -> rusqlite::Result<Article> {
        let now = now();
        let connection = self.connection();
        connection.execute(
            "INSERT INTO articles(slug, title_en, title_ru, body_en, body_ru, published, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(slug) DO UPDATE SET title_en = excluded.title_en,
             title_ru = excluded.title_ru, body_en = excluded.body_en,
             body_ru = excluded.body_ru, published = excluded.published,
             updated_at = excluded.updated_at",
            params![
                article.slug,
                article.title_en,
                article.title_ru,
                article.body_en,
                article.body_ru,
                article.published,
                now
            ],
        )?;
        connection.query_row(
            "SELECT id, slug, title_en, title_ru, body_en, body_ru, published, updated_at
             FROM articles WHERE slug = ?1",
            [&article.slug],
            |row| {
                Ok(Article {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    title_en: row.get(2)?,
                    title_ru: row.get(3)?,
                    body_en: row.get(4)?,
                    body_ru: row.get(5)?,
                    published: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
    }

    fn cleanup(&self) -> rusqlite::Result<()> {
        let now = now();
        let connection = self.connection();
        connection.execute("DELETE FROM web_sessions WHERE expires_at <= ?1", [now])?;
        connection.execute(
            "DELETE FROM presence WHERE last_seen < ?1",
            [now - PRESENCE_TTL_SECONDS],
        )?;
        Ok(())
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after Unix epoch")
        .as_secs() as i64
}

fn hash_token(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{ArticleInput, Database};

    #[test]
    fn sessions_presence_and_articles_share_one_database() {
        let db = Database::open(":memory:").expect("database");
        db.create_web_session("secret").expect("session");
        assert!(db.touch_web_session("secret").expect("touch"));
        assert!(db.owner_online().expect("presence"));

        db.save_article(&ArticleInput {
            slug: "hello".into(),
            title_en: "Hello".into(),
            title_ru: "Привет".into(),
            body_en: "Text".into(),
            body_ru: "Текст".into(),
            published: true,
        })
        .expect("article");
        assert_eq!(db.list_articles(false).expect("articles").len(), 1);
    }
}
