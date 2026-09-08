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
    pub external_avatar_url: Option<String>,
    pub avatar_revision: Option<i64>,
}

impl User {
    #[must_use]
    pub fn avatar_url(&self) -> Option<String> {
        self.avatar_revision.map_or_else(
            || self.external_avatar_url.clone(),
            |revision| Some(format!("/api/users/{}/avatar?v={revision}", self.id)),
        )
    }
}

#[derive(Debug, Clone)]
pub struct UserCredentials {
    pub user: User,
    pub password_hash: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TelegramLoginAttempt {
    pub code_verifier: String,
    pub nonce: String,
    pub next_path: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UserAvatar {
    pub bytes: Vec<u8>,
    pub content_type: String,
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
            CREATE TABLE IF NOT EXISTS telegram_login_attempts (
                state_hash TEXT PRIMARY KEY,
                code_verifier TEXT NOT NULL,
                nonce TEXT NOT NULL,
                next_path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_telegram_login_attempts_expires
                ON telegram_login_attempts(expires_at);
            ",
        )?;
        ensure_column(&connection, "users", "telegram_id", "TEXT")?;
        ensure_column(&connection, "users", "external_avatar_url", "TEXT")?;
        ensure_column(&connection, "users", "avatar_bytes", "BLOB")?;
        ensure_column(&connection, "users", "avatar_content_type", "TEXT")?;
        ensure_column(&connection, "users", "avatar_revision", "INTEGER")?;
        connection.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_users_telegram_id
             ON users(telegram_id) WHERE telegram_id IS NOT NULL",
            [],
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
            external_avatar_url: None,
            avatar_revision: None,
        })
    }

    pub fn user_credentials(&self, username: &str) -> rusqlite::Result<Option<UserCredentials>> {
        self.connection()
            .query_row(
                "SELECT id, username, password_hash, external_avatar_url, avatar_revision
                 FROM users WHERE username = ?1 COLLATE NOCASE",
                [username],
                |row| {
                    Ok(UserCredentials {
                        user: User {
                            id: row.get(0)?,
                            username: row.get(1)?,
                            external_avatar_url: row.get(3)?,
                            avatar_revision: row.get(4)?,
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
                "SELECT users.id, users.username, users.external_avatar_url,
                        users.avatar_revision
                 FROM user_sessions
                 JOIN users ON users.id = user_sessions.user_id
                 WHERE user_sessions.token_hash = ?1 AND user_sessions.expires_at > ?2",
                params![token_hash, timestamp],
                |row| {
                    Ok(User {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        external_avatar_url: row.get(2)?,
                        avatar_revision: row.get(3)?,
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

    pub fn upsert_telegram_user(
        &self,
        telegram_id: &str,
        preferred_username: Option<&str>,
        external_avatar_url: Option<&str>,
    ) -> rusqlite::Result<User> {
        let mut connection = self.connection();
        let transaction = connection.transaction()?;
        let existing = transaction
            .query_row(
                "SELECT id, username, external_avatar_url, avatar_revision
                 FROM users WHERE telegram_id = ?1",
                [telegram_id],
                user_from_row,
            )
            .optional()?;
        let username = available_telegram_username(
            &transaction,
            telegram_id,
            preferred_username,
            existing.as_ref().map(|user| user.id),
        )?;
        let user = if let Some(mut user) = existing {
            transaction.execute(
                "UPDATE users SET username = ?1, external_avatar_url = ?2 WHERE id = ?3",
                params![username, external_avatar_url, user.id],
            )?;
            user.username = username;
            user.external_avatar_url = external_avatar_url.map(str::to_owned);
            user
        } else {
            transaction.execute(
                "INSERT INTO users(
                    username, password_hash, telegram_id, external_avatar_url, created_at
                 ) VALUES (?1, '', ?2, ?3, ?4)",
                params![username, telegram_id, external_avatar_url, now()],
            )?;
            User {
                id: transaction.last_insert_rowid(),
                username,
                external_avatar_url: external_avatar_url.map(str::to_owned),
                avatar_revision: None,
            }
        };
        transaction.commit()?;
        Ok(user)
    }

    pub fn set_user_avatar(
        &self,
        user_id: i64,
        bytes: &[u8],
        content_type: &str,
    ) -> rusqlite::Result<User> {
        let connection = self.connection();
        let changed = connection.execute(
            "UPDATE users
             SET avatar_bytes = ?1, avatar_content_type = ?2,
                 avatar_revision = COALESCE(avatar_revision, 0) + 1
             WHERE id = ?3",
            params![bytes, content_type, user_id],
        )?;
        if changed != 1 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        connection.query_row(
            "SELECT id, username, external_avatar_url, avatar_revision
             FROM users WHERE id = ?1",
            [user_id],
            user_from_row,
        )
    }

    pub fn user_avatar(&self, user_id: i64) -> rusqlite::Result<Option<UserAvatar>> {
        self.connection()
            .query_row(
                "SELECT avatar_bytes, avatar_content_type
                 FROM users WHERE id = ?1 AND avatar_bytes IS NOT NULL",
                [user_id],
                |row| {
                    Ok(UserAvatar {
                        bytes: row.get(0)?,
                        content_type: row.get(1)?,
                    })
                },
            )
            .optional()
    }

    pub fn create_telegram_login_attempt(
        &self,
        state: &str,
        code_verifier: &str,
        nonce: &str,
        next_path: &str,
    ) -> rusqlite::Result<()> {
        let timestamp = now();
        let connection = self.connection();
        connection.execute(
            "DELETE FROM telegram_login_attempts WHERE expires_at <= ?1",
            [timestamp],
        )?;
        connection.execute(
            "INSERT INTO telegram_login_attempts(
                state_hash, code_verifier, nonce, next_path, created_at, expires_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                hash_token(state),
                code_verifier,
                nonce,
                next_path,
                timestamp,
                timestamp + 600
            ],
        )?;
        Ok(())
    }

    pub fn consume_telegram_login_attempt(
        &self,
        state: &str,
    ) -> rusqlite::Result<Option<TelegramLoginAttempt>> {
        let mut connection = self.connection();
        let transaction = connection.transaction()?;
        let state_hash = hash_token(state);
        let attempt = transaction
            .query_row(
                "SELECT code_verifier, nonce, next_path
                 FROM telegram_login_attempts
                 WHERE state_hash = ?1 AND expires_at > ?2",
                params![state_hash, now()],
                |row| {
                    Ok(TelegramLoginAttempt {
                        code_verifier: row.get(0)?,
                        nonce: row.get(1)?,
                        next_path: row.get(2)?,
                    })
                },
            )
            .optional()?;
        transaction.execute(
            "DELETE FROM telegram_login_attempts WHERE state_hash = ?1",
            [state_hash],
        )?;
        transaction.commit()?;
        Ok(attempt)
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

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> rusqlite::Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|existing| existing == column) {
        connection.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

fn user_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get(0)?,
        username: row.get(1)?,
        external_avatar_url: row.get(2)?,
        avatar_revision: row.get(3)?,
    })
}

fn available_telegram_username(
    connection: &Connection,
    telegram_id: &str,
    preferred_username: Option<&str>,
    current_user_id: Option<i64>,
) -> rusqlite::Result<String> {
    let sanitized = preferred_username
        .unwrap_or_default()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(24)
        .collect::<String>();
    let fallback_suffix = telegram_id
        .chars()
        .filter(char::is_ascii_digit)
        .rev()
        .take(12)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    // `svetsec` remains unavailable to password registration, but is safe here:
    // this path only receives a username from a verified Telegram ID token.
    let base = if (3..=24).contains(&sanitized.len())
        && !matches!(sanitized.to_ascii_lowercase().as_str(), "guest" | "owner")
    {
        sanitized
    } else {
        format!("telegram_{fallback_suffix}")
            .chars()
            .take(24)
            .collect()
    };
    for suffix in 0..10_000_u32 {
        let suffix = (suffix > 0).then(|| format!("_{suffix}"));
        let suffix_len = suffix.as_ref().map_or(0, String::len);
        let mut candidate = base
            .chars()
            .take(24_usize.saturating_sub(suffix_len))
            .collect::<String>();
        if let Some(suffix) = suffix {
            candidate.push_str(&suffix);
        }
        let exists = match current_user_id {
            Some(user_id) => connection
                .query_row(
                    "SELECT 1 FROM users
                     WHERE username = ?1 COLLATE NOCASE AND id != ?2",
                    params![candidate, user_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some(),
            None => connection
                .query_row(
                    "SELECT 1 FROM users WHERE username = ?1 COLLATE NOCASE",
                    [&candidate],
                    |_| Ok(()),
                )
                .optional()?
                .is_some(),
        };
        if !exists {
            return Ok(candidate);
        }
    }
    Err(rusqlite::Error::InvalidQuery)
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
    use super::{ArticleInput, CommentAuthor, CreateCommentError, Database, TelegramLoginAttempt};

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

    #[test]
    fn telegram_users_attempts_and_uploaded_avatars_are_persistent() {
        let db = Database::open(":memory:").expect("database");
        db.create_telegram_login_attempt("state", "verifier", "nonce", "/articles/hello")
            .expect("create login attempt");
        assert_eq!(
            db.consume_telegram_login_attempt("state")
                .expect("consume login attempt"),
            Some(TelegramLoginAttempt {
                code_verifier: "verifier".into(),
                nonce: "nonce".into(),
                next_path: "/articles/hello".into(),
            })
        );
        assert_eq!(
            db.consume_telegram_login_attempt("state")
                .expect("attempt is one-time"),
            None
        );

        let user = db
            .upsert_telegram_user("123456789", None, Some("https://example.com/avatar.jpg"))
            .expect("Telegram user");
        assert_eq!(user.username, "telegram_123456789");
        assert_eq!(
            user.avatar_url().as_deref(),
            Some("https://example.com/avatar.jpg")
        );
        let same_user = db
            .upsert_telegram_user("123456789", Some("svetsec"), None)
            .expect("existing Telegram user");
        assert_eq!(same_user.id, user.id);
        assert_eq!(same_user.username, "svetsec");

        let updated = db
            .set_user_avatar(user.id, b"normalized-image", "image/jpeg")
            .expect("uploaded avatar");
        assert_eq!(
            updated.avatar_url().as_deref(),
            Some(format!("/api/users/{}/avatar?v=1", user.id).as_str())
        );
        let avatar = db
            .user_avatar(user.id)
            .expect("avatar query")
            .expect("avatar");
        assert_eq!(avatar.bytes, b"normalized-image");
        assert_eq!(avatar.content_type, "image/jpeg");
    }
}
