use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SESSION_TTL_SECONDS: i64 = 60 * 60 * 24 * 30;
const COMMENT_COOLDOWN_SECONDS: i64 = 10;

#[derive(Clone)]
pub struct Database(Arc<Mutex<Connection>>);

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct UserCredentials {
    pub user: User,
    pub password_hash: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Comment {
    pub id: i64,
    pub article_slug: String,
    pub author: String,
    pub owner: bool,
    pub body: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CommentAuthor {
    Owner,
    User(i64),
}

#[derive(Debug)]
pub enum CreateCommentError {
    RateLimited,
    Database(rusqlite::Error),
}

impl From<rusqlite::Error> for CreateCommentError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

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
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL COLLATE NOCASE UNIQUE,
                password_hash TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS user_sessions (
                token_hash TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS comments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                article_slug TEXT NOT NULL,
                user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
                owner INTEGER NOT NULL DEFAULT 0 CHECK (owner IN (0, 1)),
                body TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                CHECK ((owner = 1 AND user_id IS NULL) OR (owner = 0 AND user_id IS NOT NULL))
            );
            CREATE INDEX IF NOT EXISTS idx_articles_updated_at ON articles(updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_user_sessions_expires_at ON user_sessions(expires_at);
            CREATE INDEX IF NOT EXISTS idx_comments_article_created
                ON comments(article_slug, created_at DESC, id DESC);
            ",
        )?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    pub fn create_web_session(&self, token: &str) -> rusqlite::Result<()> {
        let timestamp = now();
        self.connection().execute(
            "INSERT INTO web_sessions(token_hash, created_at, last_seen, expires_at)
             VALUES (?1, ?2, ?2, ?3)",
            params![
                hash_token(token),
                timestamp,
                timestamp + SESSION_TTL_SECONDS
            ],
        )?;
        Ok(())
    }

    pub fn touch_web_session(&self, token: &str) -> rusqlite::Result<bool> {
        let timestamp = now();
        let changed = self.connection().execute(
            "UPDATE web_sessions SET last_seen = ?1
             WHERE token_hash = ?2 AND expires_at > ?1",
            params![timestamp, hash_token(token)],
        )?;
        Ok(changed == 1)
    }

    pub fn is_web_session(&self, token: &str) -> rusqlite::Result<bool> {
        let timestamp = now();
        self.connection()
            .query_row(
                "SELECT 1 FROM web_sessions WHERE token_hash = ?1 AND expires_at > ?2",
                params![hash_token(token), timestamp],
                |_| Ok(true),
            )
            .optional()
            .map(|value| value.unwrap_or(false))
    }

    pub fn delete_web_session(&self, token: &str) -> rusqlite::Result<()> {
        self.connection().execute(
            "DELETE FROM web_sessions WHERE token_hash = ?1",
            [hash_token(token)],
        )?;
        Ok(())
    }

    pub fn create_user(&self, username: &str, password_hash: &str) -> rusqlite::Result<User> {
        let connection = self.connection();
        connection.execute(
            "INSERT INTO users(username, password_hash, created_at) VALUES (?1, ?2, ?3)",
            params![username, password_hash, now()],
        )?;
        Ok(User {
            id: connection.last_insert_rowid(),
            username: username.to_owned(),
        })
    }

    pub fn user_credentials(&self, username: &str) -> rusqlite::Result<Option<UserCredentials>> {
        self.connection()
            .query_row(
                "SELECT id, username, password_hash FROM users WHERE username = ?1 COLLATE NOCASE",
                [username],
                |row| {
                    Ok(UserCredentials {
                        user: User {
                            id: row.get(0)?,
                            username: row.get(1)?,
                        },
                        password_hash: row.get(2)?,
                    })
                },
            )
            .optional()
    }

    pub fn create_user_session(&self, token: &str, user_id: i64) -> rusqlite::Result<()> {
        let timestamp = now();
        self.connection().execute(
            "INSERT INTO user_sessions(token_hash, user_id, created_at, last_seen, expires_at)
             VALUES (?1, ?2, ?3, ?3, ?4)",
            params![
                hash_token(token),
                user_id,
                timestamp,
                timestamp + SESSION_TTL_SECONDS
            ],
        )?;
        Ok(())
    }

    pub fn user_for_session(&self, token: &str, touch: bool) -> rusqlite::Result<Option<User>> {
        self.cleanup_sessions()?;
        let timestamp = now();
        let token_hash = hash_token(token);
        let connection = self.connection();
        if touch {
            connection.execute(
                "UPDATE user_sessions SET last_seen = ?1
                 WHERE token_hash = ?2 AND expires_at > ?1",
                params![timestamp, token_hash],
            )?;
        }
        connection
            .query_row(
                "SELECT users.id, users.username
                 FROM user_sessions
                 JOIN users ON users.id = user_sessions.user_id
                 WHERE user_sessions.token_hash = ?1 AND user_sessions.expires_at > ?2",
                params![token_hash, timestamp],
                |row| {
                    Ok(User {
                        id: row.get(0)?,
                        username: row.get(1)?,
                    })
                },
            )
            .optional()
    }

    pub fn delete_user_session(&self, token: &str) -> rusqlite::Result<()> {
        self.connection().execute(
            "DELETE FROM user_sessions WHERE token_hash = ?1",
            [hash_token(token)],
        )?;
        Ok(())
    }

    pub fn list_comments(&self, article_slug: &str) -> rusqlite::Result<Vec<Comment>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT comments.id, comments.article_slug,
                    CASE WHEN comments.owner = 1 THEN 'svetsec' ELSE users.username END,
                    comments.owner, comments.body, comments.created_at
             FROM comments
             LEFT JOIN users ON users.id = comments.user_id
             WHERE comments.article_slug = ?1
             ORDER BY comments.created_at DESC, comments.id DESC
             LIMIT 100",
        )?;
        statement
            .query_map([article_slug], comment_from_row)?
            .collect()
    }

    pub fn create_comment(
        &self,
        article_slug: &str,
        author: CommentAuthor,
        body: &str,
    ) -> Result<Comment, CreateCommentError> {
        let timestamp = now();
        let connection = self.connection();
        let (user_id, owner) = match author {
            CommentAuthor::Owner => (None, true),
            CommentAuthor::User(user_id) => (Some(user_id), false),
        };
        let previous: Option<i64> = connection
            .query_row(
                "SELECT created_at FROM comments
                 WHERE (owner = ?1 AND ?1 = 1) OR (owner = 0 AND user_id = ?2)
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                params![owner, user_id],
                |row| row.get(0),
            )
            .optional()?;
        if previous.is_some_and(|previous| timestamp - previous < COMMENT_COOLDOWN_SECONDS) {
            return Err(CreateCommentError::RateLimited);
        }
        connection.execute(
            "INSERT INTO comments(article_slug, user_id, owner, body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![article_slug, user_id, owner, body, timestamp],
        )?;
        connection
            .query_row(
                "SELECT comments.id, comments.article_slug,
                        CASE WHEN comments.owner = 1 THEN 'svetsec' ELSE users.username END,
                        comments.owner, comments.body, comments.created_at
                 FROM comments
                 LEFT JOIN users ON users.id = comments.user_id
                 WHERE comments.id = ?1",
                [connection.last_insert_rowid()],
                comment_from_row,
            )
            .map_err(Into::into)
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
        let timestamp = now();
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
                timestamp
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

    fn cleanup_sessions(&self) -> rusqlite::Result<()> {
        let timestamp = now();
        let connection = self.connection();
        connection.execute(
            "DELETE FROM web_sessions WHERE expires_at <= ?1",
            [timestamp],
        )?;
        connection.execute(
            "DELETE FROM user_sessions WHERE expires_at <= ?1",
            [timestamp],
        )?;
        Ok(())
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn comment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: row.get(0)?,
        article_slug: row.get(1)?,
        author: row.get(2)?,
        owner: row.get(3)?,
        body: row.get(4)?,
        created_at: row.get(5)?,
    })
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
    use super::{ArticleInput, CommentAuthor, CreateCommentError, Database};

    #[test]
    fn owner_sessions_users_comments_and_articles_share_one_database() {
        let db = Database::open(":memory:").expect("database");
        db.create_web_session("secret").expect("session");
        assert!(db.touch_web_session("secret").expect("touch"));

        let user = db.create_user("Reader", "hash").expect("user");
        assert_eq!(
            db.user_credentials("reader")
                .expect("credentials")
                .expect("registered user")
                .user,
            user
        );
        db.create_user_session("reader-secret", user.id)
            .expect("user session");
        assert_eq!(
            db.user_for_session("reader-secret", true).expect("lookup"),
            Some(user.clone())
        );

        let comment = db
            .create_comment("hello", CommentAuthor::User(user.id), "First!")
            .expect("comment");
        assert_eq!(comment.author, "Reader");
        assert_eq!(db.list_comments("hello").expect("comments"), [comment]);
        assert!(matches!(
            db.create_comment("hello", CommentAuthor::User(user.id), "Too soon"),
            Err(CreateCommentError::RateLimited)
        ));

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

        db.delete_user_session("reader-secret")
            .expect("delete session");
        assert_eq!(
            db.user_for_session("reader-secret", false).expect("lookup"),
            None
        );
    }
}
