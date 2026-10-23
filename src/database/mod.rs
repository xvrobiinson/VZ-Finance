//! SQLite storage: connection setup, versioned migrations, backup/restore.

pub mod repositories;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use rusqlite::{Connection, OpenFlags, params};

/// Ordered, append-only list of migrations. Never edit a shipped migration;
/// add a new one instead.
const MIGRATIONS: &[(i64, &str)] =
    &[(1, include_str!("migrations/0001_init.sql")), (2, include_str!("migrations/0002_period_rate_loans.sql"))];

/// Migrations that rebuild tables and must run with foreign keys disabled.
const REBUILDS: &[i64] = &[2];

pub const SCHEMA_VERSION: i64 = 2;

/// The tables a valid VZ Finance database must contain.
const REQUIRED_TABLES: &[&str] = &[
    "schema_migrations",
    "app_settings",
    "clients",
    "loans",
    "payments",
    "payment_allocations",
    "capital_transactions",
    "loan_status_events",
];

/// `%LOCALAPPDATA%\VZFinance\data\portfolio.db`, or `VZ_FINANCE_DB` when set
/// (used for development and demos so real data is never touched).
pub fn default_db_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("VZ_FINANCE_DB").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let base = dirs::data_local_dir().context("Could not locate the local application data folder")?;
    Ok(base.join("VZFinance").join("data").join("portfolio.db"))
}

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("Could not create {}", dir.display()))?;
    }
    let mut conn = Connection::open(path).with_context(|| format!("Could not open {}", path.display()))?;
    configure(&conn)?;
    let current = schema_version(&conn)?;
    if current > 0 && current < SCHEMA_VERSION {
        // Keep a copy of the data exactly as it was before the upgrade.
        let dir = path.parent().and_then(Path::parent).unwrap_or(Path::new(".")).join("backups");
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let copy = dir.join(format!("before-upgrade-v{current}-{stamp}.db"));
        backup_to(&conn, &copy).context("Could not save a copy of your data before upgrading it")?;
        log::info!("saved pre-upgrade copy (schema {current})");
    }
    migrate(&mut conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // WAL keeps readers and the writer from blocking each other and survives
    // crashes well. In-memory databases report "memory" and that is fine.
    let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if fk != 1 {
        bail!("SQLite foreign key enforcement is unavailable");
    }
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    let exists: bool =
        conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations')", [], |r| r.get(0))?;
    if !exists {
        return Ok(0);
    }
    Ok(conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| r.get(0))?)
}

pub fn migrate(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER PRIMARY KEY NOT NULL,
            applied_at TEXT NOT NULL
        ) STRICT;",
    )?;
    let current = schema_version(conn)?;
    if current > SCHEMA_VERSION {
        bail!(
            "This database was created by a newer version of VZ Finance (schema {current}). \
             Update the app to open it; nothing was changed."
        );
    }
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
        let rebuild = REBUILDS.contains(version);
        if rebuild {
            // Required by SQLite's table-rebuild procedure; cannot be changed
            // inside a transaction.
            conn.pragma_update(None, "foreign_keys", "OFF")?;
        }
        let result = (|| -> Result<()> {
            let tx = conn.transaction()?;
            tx.execute_batch(sql).with_context(|| format!("Migration {version} failed"))?;
            if rebuild {
                let broken: i64 = tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
                if broken > 0 {
                    bail!("Migration {version} would leave {broken} broken references; nothing was changed");
                }
            }
            tx.execute("INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)", params![version, now_timestamp()])?;
            tx.commit()?;
            Ok(())
        })();
        if rebuild {
            conn.pragma_update(None, "foreign_keys", "ON")?;
        }
        result?;
        log::info!("applied migration {version}");
    }
    Ok(())
}

pub fn now_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Writes a consistent copy of the live database to `dest` using SQLite's
/// online backup API, then verifies the copy.
pub fn backup_to(conn: &Connection, dest: &Path) -> Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = dest.with_extension("partial");
    let _ = std::fs::remove_file(&tmp);
    {
        let mut out = Connection::open(&tmp)?;
        let backup = rusqlite::backup::Backup::new(conn, &mut out)?;
        backup.run_to_completion(256, std::time::Duration::from_millis(0), None)?;
    }
    // A backup file must be a self-contained single file.
    {
        let out = Connection::open(&tmp)?;
        out.pragma_update(None, "journal_mode", "DELETE")?;
    }
    validate_backup(&tmp).context("The backup could not be verified")?;
    std::fs::rename(&tmp, dest).with_context(|| format!("Could not write {}", dest.display()))?;
    Ok(())
}

/// Summary of a validated backup file, shown before a restore.
#[derive(Clone, Debug)]
pub struct BackupInfo {
    pub schema_version: i64,
    pub clients: i64,
    pub loans: i64,
    pub payments: i64,
}

/// Opens `path` read-only and checks that it is an intact VZ Finance
/// database this version can use.
pub fn validate_backup(path: &Path) -> Result<BackupInfo> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .context("The file is not a readable SQLite database")?;
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).context("The file is not a SQLite database")?;
    if integrity != "ok" {
        bail!("The database failed its integrity check: {integrity}");
    }
    for table in REQUIRED_TABLES {
        let exists: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)", [table], |r| r.get(0))?;
        if !exists {
            bail!("This is not a VZ Finance database (missing table {table})");
        }
    }
    let version = schema_version(&conn)?;
    if version > SCHEMA_VERSION {
        bail!("This backup was made by a newer version of VZ Finance (schema {version})");
    }
    let fk_problems: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
    if fk_problems > 0 {
        bail!("The backup has {fk_problems} broken references");
    }
    let count = |t: &str| -> Result<i64> { Ok(conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))?) };
    Ok(BackupInfo { schema_version: version, clients: count("clients")?, loans: count("loans")?, payments: count("payments")? })
}

/// Replaces the live database with a validated backup. A safety copy of the
/// current data is written first and its path returned.
pub fn restore_from(conn: &mut Connection, source: &Path, safety_dir: &Path) -> Result<PathBuf> {
    validate_backup(source)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let safety = safety_dir.join(format!("before-restore-{stamp}.db"));
    backup_to(conn, &safety).context("Could not save a safety copy of the current data")?;
    conn.restore(rusqlite::MAIN_DB, source, None::<fn(rusqlite::backup::Progress)>).context("Restore failed; your data was not changed")?;
    configure(conn)?;
    migrate(conn)?;
    Ok(safety)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_fresh_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.db");
        let conn = open(&path).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        drop(conn);
        let conn = open(&path).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn refuses_newer_schema() {
        let mut conn = open_in_memory().unwrap();
        conn.execute("INSERT INTO schema_migrations VALUES (99, 'x')", []).unwrap();
        let err = migrate(&mut conn).unwrap_err();
        assert!(err.to_string().contains("newer version"));
    }

    #[test]
    fn constraints_reject_bad_rows() {
        let conn = open_in_memory().unwrap();
        assert!(conn.execute("INSERT INTO clients (full_name, created_at) VALUES ('  ', 'x')", []).is_err());
        conn.execute("INSERT INTO clients (full_name, created_at) VALUES ('Ana Ruiz', 'x')", []).unwrap();
        let insert = |client: i64, rate: i64, freq: &str| {
            conn.execute(
                "INSERT INTO loans (client_id, principal, period_rate, frequency, start_date, created_at)
                 VALUES (?1, 100000, ?2, ?3, '2026-01-01', 'x')",
                params![client, rate, freq],
            )
        };
        assert!(insert(1, 100_000, "monthly").is_ok());
        assert!(insert(1, 2_000_000, "monthly").is_err(), "rate above 100%");
        assert!(insert(1, 100_000, "weekly").is_err(), "unknown frequency");
        assert!(insert(42, 100_000, "monthly").is_err(), "foreign keys are enforced");
    }

    #[test]
    fn upgrades_a_version_1_database_and_keeps_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let path = data.join("portfolio.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY NOT NULL, applied_at TEXT NOT NULL) STRICT;")
                .unwrap();
            conn.execute_batch(MIGRATIONS[0].1).unwrap();
            conn.execute_batch(
                "INSERT INTO schema_migrations VALUES (1, 'x');
                 INSERT INTO clients (full_name, created_at) VALUES ('Ana Ruiz', 'x');
                 INSERT INTO loans (client_id, principal, annual_rate, method, frequency, term_count, start_date, maturity_date, created_at)
                   VALUES (1, 100000, 1200000, 'interest_only', 'monthly', 6, '2026-01-01', '2026-07-01', 'x');
                 INSERT INTO loan_schedules (loan_id, seq, due_date, principal, interest) VALUES (1, 1, '2026-02-01', 0, 10000);
                 INSERT INTO payments (loan_id, paid_on, amount, created_at) VALUES (1, '2026-02-01', 10000, 'x');
                 INSERT INTO payment_allocations (payment_id, principal, interest, fee) VALUES (1, 0, 10000, 0);",
            )
            .unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 2);
        let (rate, freq): (i64, String) =
            conn.query_row("SELECT period_rate, frequency FROM loans WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((rate, freq.as_str()), (100_000, "monthly"));
        let payments: i64 = conn.query_row("SELECT COUNT(*) FROM payments WHERE loan_id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(payments, 1);
        let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
        let copies: Vec<_> = std::fs::read_dir(dir.path().join("backups")).unwrap().collect();
        assert_eq!(copies.len(), 1);
    }

    #[test]
    fn backup_and_restore_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open(&dir.path().join("live.db")).unwrap();
        conn.execute("INSERT INTO clients (full_name, created_at) VALUES ('Ana Ruiz', 'x')", []).unwrap();
        let backup = dir.path().join("backup.db");
        backup_to(&conn, &backup).unwrap();
        let info = validate_backup(&backup).unwrap();
        assert_eq!(info.clients, 1);

        conn.execute("INSERT INTO clients (full_name, created_at) VALUES ('Later Client', 'x')", []).unwrap();
        let safety = restore_from(&mut conn, &backup, dir.path()).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM clients", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        assert_eq!(validate_backup(&safety).unwrap().clients, 2);
    }

    #[test]
    fn rejects_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.db");
        std::fs::write(&junk, b"not a database at all, just text").unwrap();
        assert!(validate_backup(&junk).is_err());
        let other = dir.path().join("other.db");
        Connection::open(&other).unwrap().execute_batch("CREATE TABLE t (x)").unwrap();
        assert!(validate_backup(&other).unwrap_err().to_string().contains("not a VZ Finance database"));
    }
}
