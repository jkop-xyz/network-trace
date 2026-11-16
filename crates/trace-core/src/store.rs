//! Session storage: one SQLite file per Session in a sessions directory.
//!
//! A file per Session makes per-Session disk usage a file size, deletion a
//! file removal, and keeps concurrently running Sessions from contending on
//! one database.

use crate::model::{HopResult, Millis, Sample, SessionId, SessionMeta, now_ms};
use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::net::IpAddr;
use std::path::{Path, PathBuf};

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS meta (
    id          TEXT NOT NULL,
    target      TEXT NOT NULL,
    address     TEXT NOT NULL,
    started_at  INTEGER NOT NULL,
    stopped_at  INTEGER,
    interval_ms INTEGER NOT NULL,
    timeout_ms  INTEGER NOT NULL,
    note        TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS samples (
    id           INTEGER PRIMARY KEY,
    ts           INTEGER NOT NULL,
    interval_ms  INTEGER NOT NULL,
    target_known INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS samples_ts ON samples(ts);
CREATE TABLE IF NOT EXISTS probes (
    sample_id INTEGER NOT NULL,
    hop       INTEGER NOT NULL,
    address   TEXT,
    rtt_us    INTEGER,
    -- Lost Probes only: the router that said the destination was unreachable.
    unreachable_from TEXT,
    PRIMARY KEY (sample_id, hop)
) WITHOUT ROWID;
";

/// The directory holding every Session file.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

/// Summary of a stored Session for lists and the Storage page.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSession {
    #[serde(flatten)]
    pub meta: SessionMeta,
    pub size_bytes: u64,
    pub first_sample: Option<Millis>,
    pub last_sample: Option<Millis>,
}

impl Store {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Store> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        Ok(Store { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.sqlite"))
    }

    /// Session ids are `<millis>-<8 hex>`; anything else (e.g. `../x`) is
    /// rejected before it can touch the filesystem.
    fn check_id(id: &str) -> Result<()> {
        let ok = id.split_once('-').is_some_and(|(ms, hex)| {
            !ms.is_empty() && ms.bytes().all(|b| b.is_ascii_digit()) && hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit())
        });
        if ok { Ok(()) } else { Err(anyhow!("invalid session id")) }
    }

    /// Creates a new Session file and returns a writer for it.
    pub fn create(&self, target: &str, address: IpAddr, interval_ms: u32, timeout_ms: u32) -> Result<SessionFile> {
        self.create_at(now_ms(), target, address, interval_ms, timeout_ms)
    }

    /// Like `create`, with an explicit start time (for imports and test data).
    pub fn create_at(
        &self,
        started_at: Millis,
        target: &str,
        address: IpAddr,
        interval_ms: u32,
        timeout_ms: u32,
    ) -> Result<SessionFile> {
        let id = new_id(started_at);
        let conn = Connection::open(self.path(&id))?;
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "INSERT INTO meta (id, target, address, started_at, interval_ms, timeout_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, target, address.to_string(), started_at, interval_ms, timeout_ms],
        )?;
        Ok(SessionFile { conn, id })
    }

    /// Opens an existing Session file for reading or writing.
    pub fn session(&self, id: &str) -> Result<SessionFile> {
        Self::check_id(id)?;
        let path = self.path(id);
        if !path.exists() {
            return Err(anyhow!("no session {id}"));
        }
        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&conn)?;
        Ok(SessionFile { conn, id: id.to_string() })
    }

    /// Every stored Session, newest first. Unreadable files are skipped.
    pub fn list(&self) -> Result<Vec<StoredSession>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("sqlite") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            let Ok(file) = self.session(id) else { continue };
            let Ok(meta) = file.meta() else { continue };
            let (first_sample, last_sample) = file.sample_range().unwrap_or((None, None));
            out.push(StoredSession { meta, size_bytes: self.size(id), first_sample, last_sample });
        }
        out.sort_by(|a, b| b.meta.started_at.cmp(&a.meta.started_at));
        Ok(out)
    }

    /// Bytes on disk for one Session, including SQLite's side files.
    pub fn size(&self, id: &str) -> u64 {
        if Self::check_id(id).is_err() {
            return 0;
        }
        ["", "-wal", "-shm"]
            .iter()
            .filter_map(|suffix| std::fs::metadata(self.dir.join(format!("{id}.sqlite{suffix}"))).ok())
            .map(|m| m.len())
            .sum()
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        Self::check_id(id)?;
        for suffix in ["", "-wal", "-shm"] {
            let p = self.dir.join(format!("{id}.sqlite{suffix}"));
            if p.exists() {
                std::fs::remove_file(&p).with_context(|| format!("deleting {}", p.display()))?;
            }
        }
        Ok(())
    }

    /// Marks Sessions left "running" by a crash or forced quit as stopped at
    /// their last Sample. `running` lists Sessions genuinely still active.
    pub fn recover(&self, running: &[SessionId]) -> Result<()> {
        for s in self.list()? {
            if s.meta.stopped_at.is_none() && !running.contains(&s.meta.id) {
                // One unreadable file must not block recovering the others.
                let result = self.session(&s.meta.id).and_then(|f| f.stop_at(s.last_sample.unwrap_or(s.meta.started_at)));
                if let Err(e) = result {
                    eprintln!("could not recover session {}: {e:#}", s.meta.id);
                }
            }
        }
        Ok(())
    }
}

/// Brings Session files written by older versions up to date.
fn migrate(conn: &Connection) -> Result<()> {
    let has_target_known: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('samples') WHERE name = 'target_known'")?
        .exists([])?;
    if !has_target_known {
        conn.execute_batch("ALTER TABLE samples ADD COLUMN target_known INTEGER NOT NULL DEFAULT 1")?;
    }
    let has_unreachable: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('probes') WHERE name = 'unreachable_from'")?
        .exists([])?;
    if !has_unreachable {
        conn.execute_batch("ALTER TABLE probes ADD COLUMN unreachable_from TEXT")?;
    }
    Ok(())
}

fn new_id(started_at: Millis) -> SessionId {
    // Sortable and unique enough for one user: start time plus random suffix.
    let rand: u32 = {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_i64(started_at);
        h.finish() as u32
    };
    format!("{started_at}-{rand:08x}")
}

/// An open Session file.
pub struct SessionFile {
    conn: Connection,
    id: SessionId,
}

impl SessionFile {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn meta(&self) -> Result<SessionMeta> {
        self.conn
            .query_row(
                "SELECT id, target, address, started_at, stopped_at, interval_ms, timeout_ms, note FROM meta",
                [],
                |r| {
                    let address: String = r.get(2)?;
                    Ok(SessionMeta {
                        id: r.get(0)?,
                        target: r.get(1)?,
                        address: address.parse().unwrap_or(IpAddr::from([0, 0, 0, 0])),
                        started_at: r.get(3)?,
                        stopped_at: r.get(4)?,
                        interval_ms: r.get(5)?,
                        timeout_ms: r.get(6)?,
                        note: r.get(7)?,
                    })
                },
            )
            .context("reading session meta")
    }

    pub fn append(&mut self, sample: &Sample) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO samples (ts, interval_ms, target_known) VALUES (?1, ?2, ?3)",
            params![sample.ts, sample.interval_ms, sample.target_known],
        )?;
        let sample_id = tx.last_insert_rowid();
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO probes (sample_id, hop, address, rtt_us, unreachable_from) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for h in &sample.hops {
                let unreachable = h.unreachable_from.map(|a| a.to_string());
                stmt.execute(params![sample_id, h.hop, h.address.map(|a| a.to_string()), h.rtt_us, unreachable])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn stop_at(&self, ts: Millis) -> Result<()> {
        self.conn.execute("UPDATE meta SET stopped_at = ?1 WHERE stopped_at IS NULL", params![ts])?;
        Ok(())
    }

    /// Marks a stopped Session as running again, with the current timeout.
    pub fn resume(&self, timeout_ms: u32) -> Result<()> {
        self.conn.execute("UPDATE meta SET stopped_at = NULL, timeout_ms = ?1", params![timeout_ms])?;
        Ok(())
    }

    pub fn set_note(&self, note: &str) -> Result<()> {
        self.conn.execute("UPDATE meta SET note = ?1", params![note])?;
        Ok(())
    }

    /// Records a new default interval; each Sample also stores its own.
    pub fn set_interval(&self, interval_ms: u32) -> Result<()> {
        self.conn.execute("UPDATE meta SET interval_ms = ?1", params![interval_ms])?;
        Ok(())
    }

    pub fn sample_range(&self) -> Result<(Option<Millis>, Option<Millis>)> {
        Ok(self.conn.query_row("SELECT MIN(ts), MAX(ts) FROM samples", [], |r| Ok((r.get(0)?, r.get(1)?)))?)
    }

    /// Samples with `from <= ts <= to`, oldest first, hops in order.
    pub fn samples(&self, from: Millis, to: Millis) -> Result<Vec<Sample>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.id, s.ts, s.interval_ms, s.target_known, p.hop, p.address, p.rtt_us, p.unreachable_from
             FROM samples s JOIN probes p ON p.sample_id = s.id
             WHERE s.ts BETWEEN ?1 AND ?2 ORDER BY s.ts, s.id, p.hop",
        )?;
        let mut rows = stmt.query(params![from, to])?;
        let mut out: Vec<Sample> = Vec::new();
        let mut last_id = None;
        while let Some(r) = rows.next()? {
            let id: i64 = r.get(0)?;
            if last_id != Some(id) {
                out.push(Sample { ts: r.get(1)?, interval_ms: r.get(2)?, target_known: r.get(3)?, hops: Vec::new() });
                last_id = Some(id);
            }
            let address: Option<String> = r.get(5)?;
            let unreachable: Option<String> = r.get(7)?;
            out.last_mut().unwrap().hops.push(HopResult {
                hop: r.get(4)?,
                address: address.and_then(|a| a.parse().ok()),
                rtt_us: r.get(6)?,
                unreachable_from: unreachable.and_then(|a| a.parse().ok()),
            });
        }
        Ok(out)
    }

    /// Timestamps and intervals only, for Gap detection.
    pub fn sample_times(&self, from: Millis, to: Millis) -> Result<Vec<(Millis, u32)>> {
        let mut stmt = self.conn.prepare_cached("SELECT ts, interval_ms FROM samples WHERE ts BETWEEN ?1 AND ?2 ORDER BY ts")?;
        let rows = stmt.query_map(params![from, to], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Makes every later write fail, to test error handling.
    #[doc(hidden)]
    pub fn break_for_tests(&self) {
        self.conn.execute_batch("CREATE TRIGGER fail BEFORE INSERT ON samples BEGIN SELECT RAISE(FAIL, 'broken for tests'); END;").unwrap();
    }

    /// The latest `n` Samples, newest first.
    pub fn recent_samples(&self, n: u32) -> Result<Vec<Sample>> {
        let since: Option<Millis> = self
            .conn
            .query_row("SELECT ts FROM samples ORDER BY ts DESC LIMIT 1 OFFSET ?1", params![n.saturating_sub(1)], |r| r.get(0))
            .optional()?;
        let Some(since) = since.or(self.sample_range()?.0) else { return Ok(Vec::new()) };
        let mut samples = self.samples(since, Millis::MAX)?;
        samples.reverse();
        samples.truncate(n as usize);
        Ok(samples)
    }

    pub fn last_sample(&self) -> Result<Option<Sample>> {
        let ts: Option<Millis> = self.conn.query_row("SELECT MAX(ts) FROM samples", [], |r| r.get(0)).optional()?.flatten();
        match ts {
            Some(ts) => Ok(self.samples(ts, ts)?.pop()),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ts: Millis, rtts: &[Option<u32>]) -> Sample {
        Sample {
            ts,
            interval_ms: 2500,
            target_known: true,
            hops: rtts
                .iter()
                .enumerate()
                .map(|(i, rtt)| HopResult {
                    hop: i as u8 + 1,
                    address: rtt.map(|_| IpAddr::from([10, 0, 0, i as u8 + 1])),
                    rtt_us: *rtt,
                    unreachable_from: None,
                })
                .collect(),
        }
    }

    #[test]
    fn round_trips_samples_and_meta() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut file = store.create("example.com", "93.184.215.14".parse().unwrap(), 2500, 3000).unwrap();
        file.append(&sample(1000, &[Some(500), None, Some(9000)])).unwrap();
        file.append(&sample(3500, &[Some(600), Some(7000), Some(9100)])).unwrap();
        file.set_note("evening lag").unwrap();

        let id = file.id().to_string();
        drop(file);
        let file = store.session(&id).unwrap();
        let meta = file.meta().unwrap();
        assert_eq!(meta.target, "example.com");
        assert_eq!(meta.note, "evening lag");
        assert_eq!(meta.stopped_at, None);

        let samples = file.samples(0, 10_000).unwrap();
        assert_eq!(samples, vec![sample(1000, &[Some(500), None, Some(9000)]), sample(3500, &[Some(600), Some(7000), Some(9100)])]);
        assert_eq!(file.sample_range().unwrap(), (Some(1000), Some(3500)));
    }

    #[test]
    fn recover_stops_orphaned_sessions_at_last_sample() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut orphan = store.create("a", "1.1.1.1".parse().unwrap(), 2500, 3000).unwrap();
        orphan.append(&sample(42_000, &[Some(1)])).unwrap();
        let running = store.create("b", "8.8.8.8".parse().unwrap(), 2500, 3000).unwrap();
        let (orphan_id, running_id) = (orphan.id().to_string(), running.id().to_string());

        store.recover(&[running_id.clone()]).unwrap();
        assert_eq!(store.session(&orphan_id).unwrap().meta().unwrap().stopped_at, Some(42_000));
        assert_eq!(store.session(&running_id).unwrap().meta().unwrap().stopped_at, None);
    }

    #[test]
    fn delete_removes_session_and_size_reflects_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let file = store.create("a", "1.1.1.1".parse().unwrap(), 2500, 3000).unwrap();
        let id = file.id().to_string();
        drop(file);
        assert!(store.size(&id) > 0);
        assert_eq!(store.list().unwrap().len(), 1);
        store.delete(&id).unwrap();
        assert_eq!(store.size(&id), 0);
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn resume_clears_the_stop_time_and_keeps_samples() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut file = store.create("a", "1.1.1.1".parse().unwrap(), 2500, 3000).unwrap();
        file.append(&sample(1000, &[Some(1)])).unwrap();
        file.stop_at(2000).unwrap();
        file.resume(5000).unwrap();
        let meta = file.meta().unwrap();
        assert_eq!((meta.stopped_at, meta.timeout_ms), (None, 5000));
        assert_eq!(file.samples(0, 10_000).unwrap().len(), 1);
        assert_eq!(store.list().unwrap().len(), 1, "still one Session");
    }

    #[test]
    fn rejects_ids_that_could_escape_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("sessions")).unwrap();
        std::fs::write(dir.path().join("victim.sqlite"), b"x").unwrap();
        assert!(store.delete("../victim").is_err());
        assert!(store.session("../victim").is_err());
        assert!(dir.path().join("victim.sqlite").exists());
    }

    #[test]
    fn old_files_get_the_target_known_column() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let file = store.create("a", "1.1.1.1".parse().unwrap(), 2500, 3000).unwrap();
        let id = file.id().to_string();
        // Simulate a file from v0.1.0-beta.1, before the column existed.
        file.conn().execute_batch("DROP TABLE samples; CREATE TABLE samples (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, interval_ms INTEGER NOT NULL); INSERT INTO samples (ts, interval_ms) VALUES (5, 2500);").unwrap();
        drop(file);
        let file = store.session(&id).unwrap();
        assert_eq!(file.sample_times(0, 10).unwrap(), vec![(5, 2500)]);
        let known: bool = file.conn().query_row("SELECT target_known FROM samples", [], |r| r.get(0)).unwrap();
        assert!(known);
    }

    #[test]
    fn old_files_get_the_unreachable_column() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut file = store.create("a", "1.1.1.1".parse().unwrap(), 2500, 3000).unwrap();
        file.append(&sample(5, &[Some(500), None])).unwrap();
        let id = file.id().to_string();
        // Simulate a file from v0.1.0-beta.6, before the column existed.
        file.conn()
            .execute_batch(
                "CREATE TABLE p2 (sample_id INTEGER NOT NULL, hop INTEGER NOT NULL, address TEXT, rtt_us INTEGER, PRIMARY KEY (sample_id, hop)) WITHOUT ROWID;
                 INSERT INTO p2 SELECT sample_id, hop, address, rtt_us FROM probes; DROP TABLE probes; ALTER TABLE p2 RENAME TO probes;",
            )
            .unwrap();
        drop(file);
        let mut file = store.session(&id).unwrap();
        assert_eq!(file.samples(0, 10).unwrap(), vec![sample(5, &[Some(500), None])]);
        let mut s = sample(6, &[Some(500), None]);
        s.hops[1].unreachable_from = Some("192.168.1.1".parse().unwrap());
        file.append(&s).unwrap();
        assert_eq!(file.samples(6, 6).unwrap(), vec![s]);
    }
}
