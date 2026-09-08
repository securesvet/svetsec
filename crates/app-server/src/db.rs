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
const TELEGRAM_COMMENT_MODERATOR_KEY: &str = "telegram_comment_moderator_id";

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
    pub telegram_url: Option<String>,
    pub owner: bool,
    pub body: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CommentAuthor {
    Owner,
    User(i64),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CommentReader {
    Owner,
    User(i64),
}

impl CommentReader {
    fn key(self) -> String {
        match self {
            Self::Owner => "owner".into(),
            Self::User(user_id) => format!("user:{user_id}"),
        }
    }

    const fn owner(self) -> bool {
        matches!(self, Self::Owner)
    }

    const fn user_id(self) -> Option<i64> {
        match self {
            Self::Owner => None,
            Self::User(user_id) => Some(user_id),
        }
    }
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
            CREATE TABLE IF NOT EXISTS comment_readers (
                reader_key TEXT PRIMARY KEY,
                initialized_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS comment_read_cursors (
                reader_key TEXT NOT NULL REFERENCES comment_readers(reader_key)
                    ON DELETE CASCADE,
                article_slug TEXT NOT NULL,
                last_comment_id INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (reader_key, article_slug)
            );
            CREATE INDEX IF NOT EXISTS idx_comment_read_cursors_article
                ON comment_read_cursors(article_slug, reader_key);
            CREATE TABLE IF NOT EXISTS telegram_login_attempts (
                state_hash TEXT PRIMARY KEY,
                code_verifier TEXT NOT NULL,
                nonce TEXT NOT NULL,
                next_path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS site_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_telegram_login_attempts_expires
                ON telegram_login_attempts(expires_at);
            ",
        )?;
        ensure_column(&connection, "users", "telegram_id", "TEXT")?;
        ensure_column(&connection, "users", "telegram_username", "TEXT")?;
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
        verified_username: Option<&str>,
        external_avatar_url: Option<&str>,
        claim_comment_moderator: bool,
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
        let telegram_username = verified_telegram_username(verified_username);
        let user = if let Some(mut user) = existing {
            transaction.execute(
                "UPDATE users
                 SET username = ?1, telegram_username = ?2, external_avatar_url = ?3
                 WHERE id = ?4",
                params![username, telegram_username, external_avatar_url, user.id],
            )?;
            user.username = username;
            user.external_avatar_url = external_avatar_url.map(str::to_owned);
            user
        } else {
            transaction.execute(
                "INSERT INTO users(
                    username, password_hash, telegram_id, telegram_username,
                    external_avatar_url, created_at
                 ) VALUES (?1, '', ?2, ?3, ?4, ?5)",
                params![
                    username,
                    telegram_id,
                    telegram_username,
                    external_avatar_url,
                    now()
                ],
            )?;
            User {
                id: transaction.last_insert_rowid(),
                username,
                external_avatar_url: external_avatar_url.map(str::to_owned),
                avatar_revision: None,
            }
        };
        if claim_comment_moderator {
            transaction.execute(
                "INSERT OR IGNORE INTO site_settings(key, value) VALUES (?1, ?2)",
                params![TELEGRAM_COMMENT_MODERATOR_KEY, telegram_id],
            )?;
        }
        transaction.commit()?;
        Ok(user)
    }

    pub fn user_can_moderate_comments(&self, user_id: i64) -> rusqlite::Result<bool> {
        self.connection()
            .query_row(
                "SELECT 1
                 FROM users
                 JOIN site_settings
                   ON site_settings.key = ?1
                  AND site_settings.value = users.telegram_id
                 WHERE users.id = ?2",
                params![TELEGRAM_COMMENT_MODERATOR_KEY, user_id],
                |_| Ok(true),
            )
            .optional()
            .map(Option::unwrap_or_default)
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
                    comments.owner, comments.body, comments.created_at,
                    CASE
                        WHEN comments.owner = 1 THEN 'https://t.me/svetsec'
                        WHEN users.telegram_username IS NOT NULL
                            THEN 'https://t.me/' || users.telegram_username
                        ELSE NULL
                    END
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
                        comments.owner, comments.body, comments.created_at,
                        CASE
                            WHEN comments.owner = 1 THEN 'https://t.me/svetsec'
                            WHEN users.telegram_username IS NOT NULL
                                THEN 'https://t.me/' || users.telegram_username
                            ELSE NULL
                        END
                 FROM comments
                 LEFT JOIN users ON users.id = comments.user_id
                 WHERE comments.id = ?1",
                [connection.last_insert_rowid()],
                comment_from_row,
            )
            .map_err(Into::into)
    }

    pub fn delete_comment(&self, article_slug: &str, comment_id: i64) -> rusqlite::Result<bool> {
        let changed = self.connection().execute(
            "DELETE FROM comments WHERE article_slug = ?1 AND id = ?2",
            params![article_slug, comment_id],
        )?;
        Ok(changed == 1)
    }

    pub fn unread_comment_articles(&self, reader: CommentReader) -> rusqlite::Result<Vec<String>> {
        let mut connection = self.connection();
        let transaction = connection.transaction()?;
        let reader_key = reader.key();
        initialize_comment_reader(&transaction, &reader_key)?;
        let articles = {
            let mut statement = transaction.prepare(
                "SELECT comments.article_slug
                 FROM comments
                 LEFT JOIN comment_read_cursors
                   ON comment_read_cursors.reader_key = ?1
                  AND comment_read_cursors.article_slug = comments.article_slug
                 WHERE comments.id > COALESCE(comment_read_cursors.last_comment_id, 0)
                   AND NOT (
                       (?2 = 1 AND comments.owner = 1)
                       OR (
                           ?2 = 0 AND comments.owner = 0
                           AND comments.user_id = ?3
                       )
                   )
                 GROUP BY comments.article_slug
                 ORDER BY MAX(comments.id) DESC",
            )?;
            statement
                .query_map(
                    params![reader_key, reader.owner(), reader.user_id()],
                    |row| row.get(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        transaction.commit()?;
        Ok(articles)
    }

    pub fn mark_comments_read(
        &self,
        reader: CommentReader,
        article_slug: &str,
        through_comment_id: i64,
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection();
        let transaction = connection.transaction()?;
        let reader_key = reader.key();
        initialize_comment_reader(&transaction, &reader_key)?;
        transaction.execute(
            "INSERT INTO comment_read_cursors(
                 reader_key, article_slug, last_comment_id, updated_at
             ) VALUES (
                 ?1, ?2,
                 COALESCE((
                     SELECT MAX(id) FROM comments
                     WHERE article_slug = ?2 AND id <= ?3
                 ), 0),
                 ?4
             )
             ON CONFLICT(reader_key, article_slug) DO UPDATE SET
                 last_comment_id = MAX(
                     comment_read_cursors.last_comment_id,
                     excluded.last_comment_id
                 ),
                 updated_at = excluded.updated_at",
            params![reader_key, article_slug, through_comment_id, now()],
        )?;
        transaction.commit()
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

fn initialize_comment_reader(
    transaction: &rusqlite::Transaction<'_>,
    reader_key: &str,
) -> rusqlite::Result<()> {
    let timestamp = now();
    let initialized = transaction.execute(
        "INSERT OR IGNORE INTO comment_readers(reader_key, initialized_at) VALUES (?1, ?2)",
        params![reader_key, timestamp],
    )?;
    if initialized == 1 {
        transaction.execute(
            "INSERT INTO comment_read_cursors(
                 reader_key, article_slug, last_comment_id, updated_at
             )
             SELECT ?1, article_slug, MAX(id), ?2
             FROM comments
             GROUP BY article_slug",
            params![reader_key, timestamp],
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

fn verified_telegram_username(username: Option<&str>) -> Option<String> {
    let username = username?.trim().trim_start_matches('@');
    (!username.is_empty()
        && username.len() <= 32
        && username
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_'))
    .then(|| username.to_owned())
}

fn comment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: row.get(0)?,
        article_slug: row.get(1)?,
        author: row.get(2)?,
        owner: row.get(3)?,
        body: row.get(4)?,
        created_at: row.get(5)?,
        telegram_url: row.get(6)?,
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
    use super::{
        ArticleInput, CommentAuthor, CommentReader, CreateCommentError, Database,
        TelegramLoginAttempt, verified_telegram_username,
    };

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
        assert_eq!(comment.telegram_url, None);
        assert_eq!(
            db.list_comments("hello").expect("comments"),
            std::slice::from_ref(&comment)
        );
        assert!(matches!(
            db.create_comment("hello", CommentAuthor::User(user.id), "Too soon"),
            Err(CreateCommentError::RateLimited)
        ));
        assert!(
            !db.delete_comment("other", comment.id)
                .expect("wrong article")
        );
        assert!(
            db.delete_comment("hello", comment.id)
                .expect("delete comment")
        );
        assert!(
            !db.delete_comment("hello", comment.id)
                .expect("already deleted")
        );
        assert!(db.list_comments("hello").expect("comments").is_empty());

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
            .upsert_telegram_user(
                "123456789",
                None,
                None,
                Some("https://example.com/avatar.jpg"),
                false,
            )
            .expect("Telegram user");
        assert_eq!(user.username, "telegram_123456789");
        let comment_without_public_username = db
            .create_comment("telegram", CommentAuthor::User(user.id), "Hello")
            .expect("Telegram comment");
        assert_eq!(comment_without_public_username.telegram_url, None);
        assert_eq!(
            user.avatar_url().as_deref(),
            Some("https://example.com/avatar.jpg")
        );
        let same_user = db
            .upsert_telegram_user("123456789", Some("svetsec"), Some("svetsec"), None, true)
            .expect("existing Telegram user");
        assert_eq!(same_user.id, user.id);
        assert_eq!(same_user.username, "svetsec");
        assert_eq!(
            db.list_comments("telegram").expect("updated comment")[0].telegram_url,
            Some("https://t.me/svetsec".into())
        );
        assert!(
            db.user_can_moderate_comments(same_user.id)
                .expect("moderator lookup")
        );

        let other_user = db
            .upsert_telegram_user("987654321", Some("other"), Some("other"), None, true)
            .expect("other Telegram user");
        assert!(
            !db.user_can_moderate_comments(other_user.id)
                .expect("moderator cannot be replaced")
        );
        let owner_comment = db
            .create_comment("owner", CommentAuthor::Owner, "Owner note")
            .expect("owner comment");
        assert_eq!(
            owner_comment.telegram_url.as_deref(),
            Some("https://t.me/svetsec")
        );

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

    #[test]
    fn telegram_profile_username_accepts_only_safe_public_handles() {
        assert_eq!(
            verified_telegram_username(Some(" @Secure_Svet ")),
            Some("Secure_Svet".into())
        );
        assert_eq!(verified_telegram_username(Some("display name")), None);
        assert_eq!(verified_telegram_username(Some("unsafe/path")), None);
        assert_eq!(verified_telegram_username(None), None);
    }

    #[test]
    fn comment_notifications_are_per_reader_persistent_and_ignore_own_comments() {
        let db = Database::open(":memory:").expect("database");
        let first = db.create_user("first", "hash").expect("first user");
        let second = db.create_user("second", "hash").expect("second user");
        db.create_comment("hello", CommentAuthor::User(first.id), "Existing")
            .expect("existing comment");

        assert!(
            db.unread_comment_articles(CommentReader::Owner)
                .expect("owner baseline")
                .is_empty()
        );
        assert!(
            db.unread_comment_articles(CommentReader::User(first.id))
                .expect("reader baseline")
                .is_empty()
        );
        assert!(
            db.unread_comment_articles(CommentReader::User(second.id))
                .expect("author baseline")
                .is_empty()
        );

        db.create_comment("hello", CommentAuthor::User(second.id), "New")
            .expect("new reader comment");
        assert_eq!(
            db.unread_comment_articles(CommentReader::Owner)
                .expect("owner notifications"),
            ["hello"]
        );
        assert_eq!(
            db.unread_comment_articles(CommentReader::User(first.id))
                .expect("reader notifications"),
            ["hello"]
        );
        assert!(
            db.unread_comment_articles(CommentReader::User(second.id))
                .expect("author ignores own comment")
                .is_empty()
        );

        db.mark_comments_read(CommentReader::Owner, "hello", i64::MAX)
            .expect("mark owner read");
        assert!(
            db.unread_comment_articles(CommentReader::Owner)
                .expect("read owner notifications")
                .is_empty()
        );
        assert_eq!(
            db.unread_comment_articles(CommentReader::User(first.id))
                .expect("other reader stays unread"),
            ["hello"]
        );

        db.create_comment("other", CommentAuthor::Owner, "Owner note")
            .expect("owner comment");
        assert!(
            db.unread_comment_articles(CommentReader::Owner)
                .expect("owner ignores own comments")
                .is_empty()
        );
        assert_eq!(
            db.unread_comment_articles(CommentReader::User(first.id))
                .expect("reader sees both articles"),
            ["other", "hello"]
        );
    }
}
