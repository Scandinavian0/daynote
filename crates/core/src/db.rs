use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database schema version {0} is newer than this program supports ({1})")]
    SchemaTooNew(i64, i64),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Store {
    pub(crate) conn: Connection,
}

const SCHEMA_VERSION: i64 = 2;

const MIGRATION_1: &str = r#"
CREATE TABLE tasks (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    date          TEXT    NOT NULL,
    title         TEXT    NOT NULL,
    note          TEXT,
    status        TEXT    NOT NULL DEFAULT 'todo',
    completed_at  TEXT,
    completed_by  TEXT,
    remind_at     TEXT,
    snooze_until  TEXT,
    reminded_at   TEXT,
    priority      INTEGER NOT NULL DEFAULT 0,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    parent_id     INTEGER,
    series_id     INTEGER,
    source        TEXT    NOT NULL,
    created_at    TEXT    NOT NULL,
    updated_at    TEXT    NOT NULL,
    deleted_at    TEXT
);
CREATE INDEX idx_tasks_date   ON tasks(date, sort_order);
CREATE INDEX idx_tasks_parent ON tasks(parent_id);
CREATE INDEX idx_tasks_series ON tasks(series_id);
CREATE INDEX idx_tasks_remind ON tasks(remind_at) WHERE remind_at IS NOT NULL;

CREATE TABLE day_notes (
    date       TEXT PRIMARY KEY,
    content    TEXT NOT NULL,
    source     TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE recurring (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT    NOT NULL,
    note        TEXT,
    rule        TEXT    NOT NULL,
    remind_time TEXT,
    start_date  TEXT    NOT NULL,
    end_date    TEXT,
    active      INTEGER NOT NULL DEFAULT 1,
    source      TEXT    NOT NULL,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);

CREATE TABLE recurring_generated (
    series_id INTEGER NOT NULL,
    date      TEXT    NOT NULL,
    PRIMARY KEY (series_id, date)
) WITHOUT ROWID;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

// Track the exact children hidden by a parent deletion. A wall-clock timestamp is
// not a deletion identity: independent actions can occur within the same second.
const MIGRATION_2: &str = r#"
CREATE TABLE task_delete_children (
    parent_id INTEGER NOT NULL,
    child_id  INTEGER NOT NULL,
    PRIMARY KEY (parent_id, child_id)
) WITHOUT ROWID;
INSERT INTO task_delete_children (parent_id, child_id)
SELECT parent.id, child.id
FROM tasks parent JOIN tasks child ON child.parent_id = parent.id
WHERE parent.deleted_at IS NOT NULL AND child.deleted_at = parent.deleted_at;
"#;

impl Store {
    /// Open (creating if needed) the database at `path`: WAL, busy_timeout=5000, migrations.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        // journal_mode returns a row; query it rather than execute.
        let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        conn.execute_batch("PRAGMA synchronous=NORMAL;")?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Open `data_dir()/daynote.db`.
    pub fn open_default() -> Result<Store> {
        Store::open(&crate::data_dir()?.join("daynote.db"))
    }

    pub fn open_in_memory() -> Result<Store> {
        let conn = Connection::open_in_memory()?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    /// `PRAGMA data_version`: changes when another connection commits a write.
    pub fn data_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA data_version", [], |r| r.get(0))?)
    }

    fn migrate(&mut self) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(Error::SchemaTooNew(version, SCHEMA_VERSION));
        }
        if version < 1 {
            tx.execute_batch(MIGRATION_1)?;
        }
        if version < 2 {
            tx.execute_batch(MIGRATION_2)?;
        }
        if version < SCHEMA_VERSION {
            tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
        }
        tx.commit()?;
        Ok(())
    }
}
