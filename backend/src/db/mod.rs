//! Persistence of the application state in SQLite (default: a file in the data directory) or
//! PostgreSQL (`ODB_DATABASE_URL=postgres://…`).
//!
//! Queries are written once, in SQL both engines accept, with SQLite-style `?` / `?N`
//! placeholders; they are rewritten to `$N` before reaching PostgreSQL. Values are bound
//! through [`Arg`] so a `NULL` keeps its column type on PostgreSQL. Use [`query`],
//! [`query_as`] and [`query_scalar`] like their `sqlx` namesakes, passing `&Db` or `&mut Tx`.

pub mod models;

use std::marker::PhantomData;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions, PgRow};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteRow, SqliteSynchronous,
};
use sqlx::{AssertSqlSafe, FromRow, Postgres, SqlSafeStr, SqlStr, Sqlite};

pub use models::*;

/// Connection pool of the configured database engine.
#[derive(Clone)]
pub enum Db {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

/// An open transaction; dropped without [`Tx::commit`] it rolls back.
pub enum Tx {
    Sqlite(sqlx::Transaction<'static, Sqlite>),
    Postgres(sqlx::Transaction<'static, Postgres>),
}

/// Where a query runs: the pool or an open transaction.
pub enum Conn<'c> {
    Pool(&'c Db),
    Tx(&'c mut Tx),
}

impl<'c> From<&'c Db> for Conn<'c> {
    fn from(db: &'c Db) -> Self {
        Conn::Pool(db)
    }
}

impl<'c> From<&'c mut Tx> for Conn<'c> {
    fn from(tx: &'c mut Tx) -> Self {
        Conn::Tx(tx)
    }
}

/// Opens the database named by `url` (or the SQLite file `sqlite_file` when unset) and migrates it.
pub async fn connect(url: Option<&str>, sqlite_file: &Path) -> anyhow::Result<Db> {
    match url.map(str::trim).filter(|u| !u.is_empty()) {
        None => {
            let options = SqliteConnectOptions::from_str("sqlite://")?.filename(sqlite_file);
            connect_sqlite(options, &sqlite_file.display().to_string()).await
        }
        Some(url) if url.starts_with("sqlite:") => {
            let options = SqliteConnectOptions::from_str(url).context("invalid ODB_DATABASE_URL")?;
            connect_sqlite(options, url).await
        }
        Some(url) if url.starts_with("postgres://") || url.starts_with("postgresql://") => connect_postgres(url).await,
        Some(_) => bail!("ODB_DATABASE_URL must start with postgres://, postgresql:// or sqlite:"),
    }
}

async fn connect_sqlite(options: SqliteConnectOptions, location: &str) -> anyhow::Result<Db> {
    let options = options
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(15));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .with_context(|| format!("opening database {location}"))?;
    sqlx::migrate!("./migrations/sqlite").run(&pool).await.context("running database migrations")?;
    tracing::debug!(database = %location, "using SQLite");
    Ok(Db::Sqlite(pool))
}

async fn connect_postgres(url: &str) -> anyhow::Result<Db> {
    let location = redact_url(url);
    let options = PgConnectOptions::from_str(url).context("invalid ODB_DATABASE_URL")?.application_name("odoo-backup");
    let pool_options = PgPoolOptions::new().max_connections(10).acquire_timeout(Duration::from_secs(30));
    // The server may still be starting (e.g. both containers launched together): wait up to a minute.
    let mut attempt = 0;
    let pool = loop {
        attempt += 1;
        match pool_options.clone().connect_with(options.clone()).await {
            Ok(pool) => break pool,
            Err(err) if attempt < 30 && is_transient(&err) => {
                tracing::warn!(database = %location, error = %err, "database not reachable yet, retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(err) => return Err(err).with_context(|| format!("connecting to {location}")),
        }
    };
    sqlx::migrate!("./migrations/postgres").run(&pool).await.context("running database migrations")?;
    tracing::debug!(database = %location, "using PostgreSQL");
    Ok(Db::Postgres(pool))
}

fn is_transient(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut => true,
        // 57P03: "the database system is starting up".
        sqlx::Error::Database(db) => db.code().as_deref() == Some("57P03"),
        _ => false,
    }
}

/// The URL without its password, for logs and error messages.
pub fn redact_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) => {
            if parsed.password().is_some() {
                let _ = parsed.set_password(Some("***"));
            }
            parsed.to_string()
        }
        Err(_) => "<invalid database URL>".into(),
    }
}

/// Advisory lock key held by the running server (an arbitrary constant: "odoo-bkp").
const SERVER_LOCK_KEY: i64 = 0x6f64_6f6f_2d62_6b70;

/// Keeps other servers off the database while alive (see [`Db::claim`]).
pub struct ServerClaim {
    _connection: Option<PgConnection>,
}

impl Db {
    pub fn engine(&self) -> &'static str {
        match self {
            Db::Sqlite(_) => "sqlite",
            Db::Postgres(_) => "postgres",
        }
    }

    pub async fn begin(&self) -> sqlx::Result<Tx> {
        Ok(match self {
            Db::Sqlite(pool) => Tx::Sqlite(pool.begin().await?),
            Db::Postgres(pool) => Tx::Postgres(pool.begin().await?),
        })
    }

    pub async fn close(&self) {
        match self {
            Db::Sqlite(pool) => pool.close().await,
            Db::Postgres(pool) => pool.close().await,
        }
    }

    /// Ensures a single server uses a PostgreSQL database: two schedulers would run every backup
    /// twice. The lock lives as long as the returned guard (and its connection).
    pub async fn claim(&self) -> anyhow::Result<ServerClaim> {
        let Db::Postgres(pool) = self else {
            return Ok(ServerClaim { _connection: None });
        };
        let mut connection = pool.acquire().await?.detach();
        let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(SERVER_LOCK_KEY)
            .fetch_one(&mut connection)
            .await?;
        if !locked {
            bail!(
                "another odoo-backup server is already using this PostgreSQL database; \
                 run a single instance (stop the other one first)"
            );
        }
        Ok(ServerClaim { _connection: Some(connection) })
    }

    /// Engine name, server version and on-disk size, for the system page.
    pub async fn describe(&self) -> serde_json::Value {
        let (engine, version_sql, size_sql) = match self {
            Db::Sqlite(_) => (
                "SQLite",
                "SELECT sqlite_version()",
                "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            ),
            Db::Postgres(_) => ("PostgreSQL", "SHOW server_version", "SELECT pg_database_size(current_database())"),
        };
        let version: Option<String> = query_scalar(version_sql).fetch_optional(self).await.ok().flatten();
        let size: Option<i64> = query_scalar(size_sql).fetch_optional(self).await.ok().flatten();
        serde_json::json!({ "engine": engine, "version": version, "size_bytes": size })
    }
}

impl Tx {
    pub async fn commit(self) -> sqlx::Result<()> {
        match self {
            Tx::Sqlite(tx) => tx.commit().await,
            Tx::Postgres(tx) => tx.commit().await,
        }
    }

    /// Serializes this transaction with others taking the same key, until it ends. Needed on
    /// PostgreSQL for check-then-insert sequences; SQLite already runs one writer at a time.
    pub async fn serialize(&mut self, key: i64) -> sqlx::Result<()> {
        if let Tx::Postgres(tx) = self {
            sqlx::query("SELECT pg_advisory_xact_lock($1)").bind(key).execute(&mut **tx).await?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Queries

/// A value bound to a query parameter.
#[derive(Debug, Clone)]
pub enum Arg {
    Bool(Option<bool>),
    Int(Option<i64>),
    Text(Option<String>),
    Time(Option<DateTime<Utc>>),
}

/// Types that can be bound to a query parameter.
pub trait IntoArg {
    fn into_arg(self) -> Arg;
    /// `NULL` with this type's SQL type.
    fn null() -> Arg;
}

macro_rules! into_arg {
    ($variant:ident: $($ty:ty => |$v:ident| $convert:expr),+ $(,)?) => {
        $(impl IntoArg for $ty {
            fn into_arg(self) -> Arg {
                let $v = self;
                Arg::$variant(Some($convert))
            }
            fn null() -> Arg {
                Arg::$variant(None)
            }
        })+
    };
}

into_arg!(Bool: bool => |v| v, &bool => |v| *v);
into_arg!(Int: i64 => |v| v, &i64 => |v| *v, i32 => |v| i64::from(v), u32 => |v| i64::from(v));
into_arg!(Text: String => |v| v, &String => |v| v.clone(), &str => |v| v.to_owned(), &&str => |v| (*v).to_owned());
into_arg!(Time: DateTime<Utc> => |v| v, &DateTime<Utc> => |v| *v);

impl<T: IntoArg> IntoArg for Option<T> {
    fn into_arg(self) -> Arg {
        match self {
            Some(value) => value.into_arg(),
            None => T::null(),
        }
    }
    fn null() -> Arg {
        T::null()
    }
}

impl<'a, T> IntoArg for &'a Option<T>
where
    &'a T: IntoArg,
{
    fn into_arg(self) -> Arg {
        self.as_ref().into_arg()
    }
    fn null() -> Arg {
        <&'a T>::null()
    }
}

/// Rewrites SQLite `?` / `?N` placeholders to PostgreSQL `$N` (string literals and quoted
/// identifiers are left alone).
fn rebind(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 16);
    let mut chars = sql.chars().peekable();
    let mut quote: Option<char> = None;
    let mut next = 0u32;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                out.push(c);
            }
            '?' => {
                out.push('$');
                if chars.peek().is_some_and(char::is_ascii_digit) {
                    while let Some(d) = chars.next_if(char::is_ascii_digit) {
                        out.push(d);
                    }
                } else {
                    next += 1;
                    out.push_str(&next.to_string());
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn postgres_sql(sql: SqlStr) -> SqlStr {
    AssertSqlSafe(rebind(sql.as_str())).into_sql_str()
}

/// Runs `$body` with `$db` naming the backend's `sqlx::Database`, `$sql` the SQL for it and
/// `$ex` its executor.
macro_rules! on_backend {
    ($conn:expr, $sql:expr, |$db:ident, $sql_id:ident, $ex:ident| $body:expr) => {
        match $conn {
            Conn::Pool(Db::Sqlite(pool)) => {
                type $db = Sqlite;
                let $sql_id = $sql;
                let $ex = pool;
                $body
            }
            Conn::Pool(Db::Postgres(pool)) => {
                type $db = Postgres;
                let $sql_id = postgres_sql($sql);
                let $ex = pool;
                $body
            }
            Conn::Tx(Tx::Sqlite(tx)) => {
                type $db = Sqlite;
                let $sql_id = $sql;
                let $ex = &mut **tx;
                $body
            }
            Conn::Tx(Tx::Postgres(tx)) => {
                type $db = Postgres;
                let $sql_id = postgres_sql($sql);
                let $ex = &mut **tx;
                $body
            }
        }
    };
}

macro_rules! bind_args {
    ($query:expr, $args:expr) => {{
        let mut query = $query;
        for arg in $args {
            query = match arg {
                Arg::Bool(v) => query.bind(v),
                Arg::Int(v) => query.bind(v),
                Arg::Text(v) => query.bind(v),
                Arg::Time(v) => query.bind(v),
            };
        }
        query
    }};
}

/// Result of [`Query::execute`].
#[derive(Debug)]
pub struct Done(u64);

impl Done {
    pub fn rows_affected(&self) -> u64 {
        self.0
    }
}

/// A statement without result rows (see [`query`]).
pub struct Query {
    sql: SqlStr,
    args: Vec<Arg>,
}

/// A query decoding each row into `T` (see [`query_as`]).
pub struct QueryAs<T> {
    sql: SqlStr,
    args: Vec<Arg>,
    _row: PhantomData<fn() -> T>,
}

/// A query decoding the first column of each row into `T` (see [`query_scalar`]).
pub struct QueryScalar<T> {
    sql: SqlStr,
    args: Vec<Arg>,
    _value: PhantomData<fn() -> T>,
}

pub fn query(sql: impl SqlSafeStr) -> Query {
    Query { sql: sql.into_sql_str(), args: Vec::new() }
}

pub fn query_as<T>(sql: impl SqlSafeStr) -> QueryAs<T> {
    QueryAs { sql: sql.into_sql_str(), args: Vec::new(), _row: PhantomData }
}

pub fn query_scalar<T>(sql: impl SqlSafeStr) -> QueryScalar<T> {
    QueryScalar { sql: sql.into_sql_str(), args: Vec::new(), _value: PhantomData }
}

impl Query {
    pub fn bind(mut self, value: impl IntoArg) -> Self {
        self.args.push(value.into_arg());
        self
    }

    pub async fn execute<'c>(self, conn: impl Into<Conn<'c>>) -> sqlx::Result<Done> {
        let Query { sql: text, args } = self;
        let rows = on_backend!(conn.into(), text, |DB, sql, ex| {
            bind_args!(sqlx::query::<DB>(sql), args).execute(ex).await?.rows_affected()
        });
        Ok(Done(rows))
    }
}

impl<T> QueryAs<T>
where
    T: Send + Unpin + for<'r> FromRow<'r, SqliteRow> + for<'r> FromRow<'r, PgRow>,
{
    pub fn bind(mut self, value: impl IntoArg) -> Self {
        self.args.push(value.into_arg());
        self
    }

    pub async fn fetch_one<'c>(self, conn: impl Into<Conn<'c>>) -> sqlx::Result<T> {
        let QueryAs { sql: text, args, .. } = self;
        on_backend!(conn.into(), text, |DB, sql, ex| {
            bind_args!(sqlx::query_as::<DB, T>(sql), args).fetch_one(ex).await
        })
    }

    pub async fn fetch_optional<'c>(self, conn: impl Into<Conn<'c>>) -> sqlx::Result<Option<T>> {
        let QueryAs { sql: text, args, .. } = self;
        on_backend!(conn.into(), text, |DB, sql, ex| {
            bind_args!(sqlx::query_as::<DB, T>(sql), args).fetch_optional(ex).await
        })
    }

    pub async fn fetch_all<'c>(self, conn: impl Into<Conn<'c>>) -> sqlx::Result<Vec<T>> {
        let QueryAs { sql: text, args, .. } = self;
        on_backend!(conn.into(), text, |DB, sql, ex| {
            bind_args!(sqlx::query_as::<DB, T>(sql), args).fetch_all(ex).await
        })
    }
}

impl<T> QueryScalar<T>
where
    T: Send + Unpin,
    (T,): for<'r> FromRow<'r, SqliteRow> + for<'r> FromRow<'r, PgRow>,
{
    pub fn bind(mut self, value: impl IntoArg) -> Self {
        self.args.push(value.into_arg());
        self
    }

    pub async fn fetch_one<'c>(self, conn: impl Into<Conn<'c>>) -> sqlx::Result<T> {
        let QueryScalar { sql: text, args, .. } = self;
        on_backend!(conn.into(), text, |DB, sql, ex| {
            bind_args!(sqlx::query_scalar::<DB, T>(sql), args).fetch_one(ex).await
        })
    }

    pub async fn fetch_optional<'c>(self, conn: impl Into<Conn<'c>>) -> sqlx::Result<Option<T>> {
        let QueryScalar { sql: text, args, .. } = self;
        on_backend!(conn.into(), text, |DB, sql, ex| {
            bind_args!(sqlx::query_scalar::<DB, T>(sql), args).fetch_optional(ex).await
        })
    }

    pub async fn fetch_all<'c>(self, conn: impl Into<Conn<'c>>) -> sqlx::Result<Vec<T>> {
        let QueryScalar { sql: text, args, .. } = self;
        on_backend!(conn.into(), text, |DB, sql, ex| {
            bind_args!(sqlx::query_scalar::<DB, T>(sql), args).fetch_all(ex).await
        })
    }
}

/// `(?, ?, …)` with `n` placeholders, for `IN` lists.
pub fn placeholders(n: usize) -> String {
    let mut out = String::with_capacity(2 + n * 3);
    out.push('(');
    for i in 0..n {
        if i > 0 {
            out.push_str(", ");
        }
        out.push('?');
    }
    out.push(')');
    out
}

// ---------------------------------------------------------------------------------------------
// Shared lookups

pub async fn get_setting(db: &Db, key: &str) -> sqlx::Result<Option<String>> {
    query_scalar("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(db).await
}

pub async fn set_setting(db: &Db, key: &str, value: &str) -> sqlx::Result<()> {
    query(
        "INSERT INTO settings (key, value) VALUES (?, ?) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_instance(db: &Db, id: i64) -> sqlx::Result<InstanceRow> {
    query_as("SELECT * FROM instances WHERE id = ?").bind(id).fetch_one(db).await
}

pub async fn get_destination(db: &Db, id: i64) -> sqlx::Result<DestinationRow> {
    query_as("SELECT * FROM destinations WHERE id = ?").bind(id).fetch_one(db).await
}

pub async fn get_job(db: &Db, id: i64) -> sqlx::Result<JobRow> {
    query_as("SELECT * FROM jobs WHERE id = ?").bind(id).fetch_one(db).await
}

pub async fn job_destination_ids(db: &Db, job_id: i64) -> sqlx::Result<Vec<i64>> {
    query_scalar("SELECT destination_id FROM job_destinations WHERE job_id = ? ORDER BY destination_id")
        .bind(job_id)
        .fetch_all(db)
        .await
}

pub async fn get_run(db: &Db, id: i64) -> sqlx::Result<Run> {
    query_as(concat!(crate::run_select!(), " WHERE r.id = ?")).bind(id).fetch_one(db).await
}

pub async fn get_artifact(db: &Db, id: i64) -> sqlx::Result<Artifact> {
    query_as("SELECT * FROM artifacts WHERE id = ?").bind(id).fetch_one(db).await
}

pub async fn run_artifacts(db: &Db, run_id: i64) -> sqlx::Result<Vec<Artifact>> {
    query_as("SELECT * FROM artifacts WHERE run_id = ? ORDER BY id").bind(run_id).fetch_all(db).await
}

pub async fn run_logs(db: &Db, run_id: i64) -> sqlx::Result<Vec<RunLog>> {
    query_as("SELECT * FROM run_logs WHERE run_id = ? ORDER BY id").bind(run_id).fetch_all(db).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebind_numbers_placeholders() {
        assert_eq!(rebind("SELECT * FROM t WHERE a = ? AND b = ?"), "SELECT * FROM t WHERE a = $1 AND b = $2");
        assert_eq!(rebind("WHERE (?1 IS NULL OR x = ?1) LIMIT ?12"), "WHERE ($1 IS NULL OR x = $1) LIMIT $12");
        assert_eq!(rebind("SELECT '?', \"a?\", 'it''s ?' WHERE x = ?"), "SELECT '?', \"a?\", 'it''s ?' WHERE x = $1");
    }

    #[test]
    fn placeholder_lists() {
        assert_eq!(placeholders(1), "(?)");
        assert_eq!(placeholders(3), "(?, ?, ?)");
    }

    #[test]
    fn urls_are_redacted() {
        assert_eq!(redact_url("postgres://odoo:s3cret@db:5432/backup"), "postgres://odoo:***@db:5432/backup");
        assert_eq!(redact_url("postgres://db/backup"), "postgres://db/backup");
    }

    /// Runs the SQL patterns the application relies on against one engine.
    async fn exercise(db: &Db) {
        set_setting(db, "k", "1").await.unwrap();
        set_setting(db, "k", "2").await.unwrap();
        assert_eq!(get_setting(db, "k").await.unwrap().as_deref(), Some("2"));
        assert_eq!(get_setting(db, "missing").await.unwrap(), None);

        // RETURNING, typed NULLs, booleans and timestamps.
        let ts = Utc::now();
        let id: i64 = query_scalar(
            "INSERT INTO instances (name, url, master_password_enc, verify_tls, timeout_secs, notes, \
             last_checked_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind("Main")
        .bind("https://odoo.example.com")
        .bind(None::<String>)
        .bind(false)
        .bind(600i64)
        .bind("")
        .bind(None::<DateTime<Utc>>)
        .bind(ts)
        .bind(ts)
        .fetch_one(db)
        .await
        .unwrap();
        let row = get_instance(db, id).await.unwrap();
        assert!(!row.verify_tls);
        assert_eq!(row.timeout_secs, 600);
        assert!(row.master_password_enc.is_none() && row.last_checked_at.is_none());
        assert!((row.created_at - ts).num_milliseconds().abs() <= 1);

        let duplicate = query("INSERT INTO instances (name, url, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("Main")
            .bind("https://other")
            .bind(ts)
            .bind(ts)
            .execute(db)
            .await
            .unwrap_err();
        assert!(matches!(crate::error::AppError::from(duplicate), crate::error::AppError::Conflict(_)));

        // A dropped transaction rolls back; a committed one sticks.
        {
            let mut tx = db.begin().await.unwrap();
            tx.serialize(1).await.unwrap();
            query("UPDATE instances SET notes = ? WHERE id = ?").bind("lost").bind(id).execute(&mut tx).await.unwrap();
        }
        assert_eq!(get_instance(db, id).await.unwrap().notes, "");
        let mut tx = db.begin().await.unwrap();
        let changed = query("UPDATE instances SET notes = ? WHERE id = ?")
            .bind("kept")
            .bind(id)
            .execute(&mut tx)
            .await
            .unwrap()
            .rows_affected();
        tx.commit().await.unwrap();
        assert_eq!(changed, 1);
        assert_eq!(get_instance(db, id).await.unwrap().notes, "kept");

        // Runs: aggregates, IN lists, reused numbered placeholders, case-insensitive search.
        let mut run_ids = Vec::new();
        for (database, status, size) in
            [("Prod", "success", 100i64), ("prod_copy", "failed", 0), ("test", "success", 50)]
        {
            let run_id: i64 = query_scalar(
                "INSERT INTO runs (kind, instance_id, instance_name, database, format, trigger, status, size_bytes, \
                 created_at) VALUES ('backup', ?, 'Main', ?, 'zip', 'manual', ?, ?, ?) RETURNING id",
            )
            .bind(id)
            .bind(database)
            .bind(status)
            .bind(size)
            .bind(ts)
            .fetch_one(db)
            .await
            .unwrap();
            run_ids.push(run_id);
        }
        let (ok, bytes): (i64, i64) = query_as(
            "SELECT COUNT(*) FILTER (WHERE status = 'success'), \
             CAST(COALESCE(SUM(size_bytes) FILTER (WHERE status = 'success'), 0) AS BIGINT) FROM runs",
        )
        .fetch_one(db)
        .await
        .unwrap();
        assert_eq!((ok, bytes), (2, 150));

        let mut listed = query_as::<Run>(AssertSqlSafe(format!(
            "{} WHERE r.id IN {} ORDER BY r.id",
            crate::run_select!(),
            placeholders(2)
        )));
        for run_id in &run_ids[..2] {
            listed = listed.bind(run_id);
        }
        let listed = listed.fetch_all(db).await.unwrap();
        assert_eq!(listed.iter().map(|r| r.database.as_str()).collect::<Vec<_>>(), ["Prod", "prod_copy"]);
        assert!(listed[0].job_name.is_none() && listed[0].started_at.is_none());

        let matching: i64 =
            query_scalar("SELECT COUNT(*) FROM runs WHERE (?1 IS NULL OR status = ?1) AND LOWER(database) LIKE ?2")
                .bind(None::<String>)
                .bind("%prod%")
                .fetch_one(db)
                .await
                .unwrap();
        assert_eq!(matching, 2);
        let failed: Option<String> =
            query_scalar("SELECT database FROM runs WHERE status = ?1 OR status = ?1 ORDER BY id DESC LIMIT 1")
                .bind("failed")
                .fetch_optional(db)
                .await
                .unwrap();
        assert_eq!(failed.as_deref(), Some("prod_copy"));

        let described = db.describe().await;
        assert!(described["version"].is_string() && described["size_bytes"].is_i64(), "{described}");
    }

    #[tokio::test]
    async fn sqlite_backend() {
        let dir = tempfile::tempdir().unwrap();
        let db = connect(None, &dir.path().join("test.db")).await.unwrap();
        assert_eq!(db.engine(), "sqlite");
        exercise(&db).await;
        db.close().await;
    }

    /// Runs against PostgreSQL when `ODB_TEST_POSTGRES_URL` points at a server where the user may
    /// create databases (a throwaway database is created and dropped).
    #[tokio::test]
    async fn postgres_backend() {
        use sqlx::Connection;
        let Ok(admin_url) = std::env::var("ODB_TEST_POSTGRES_URL") else {
            eprintln!("skipped: ODB_TEST_POSTGRES_URL is not set");
            return;
        };
        let name = format!("odb_test_{}", hex::encode(crate::crypto::random_bytes::<6>()));
        let mut admin = PgConnection::connect(&admin_url).await.unwrap();
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {name}"))).execute(&mut admin).await.unwrap();
        let mut url = url::Url::parse(&admin_url).unwrap();
        url.set_path(&name);

        let db = connect(Some(url.as_str()), Path::new("unused")).await.unwrap();
        assert_eq!(db.engine(), "postgres");
        let claim = db.claim().await.unwrap();
        assert!(db.claim().await.is_err(), "a second server must be refused");
        drop(claim);
        exercise(&db).await;
        db.close().await;

        sqlx::query(AssertSqlSafe(format!("DROP DATABASE {name} WITH (FORCE)"))).execute(&mut admin).await.unwrap();
    }
}
