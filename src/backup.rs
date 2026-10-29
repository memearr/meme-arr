//! Settings backups (System -> Backups), modeled on Sonarr's Backup page:
//! timestamped files you can download, and restore from.
//!
//! A backup is a small JSON file of the app's *configuration*: the settings
//! table (Plex URL/token, Authentik, the admin password hash, ...), the path
//! mappings, and task schedules. The subtitle index is deliberately not
//! included: it can be rebuilt by a library scan, and at multiple GB would
//! make every backup slow and large. The file contains secrets, so keep it
//! as private as the data volume itself.

use crate::db::{self, Mapping};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FORMAT_VERSION: u32 = 1;
/// How many backups to keep; older ones are deleted after each new one.
pub const KEEP: usize = 14;
const MAX_SETTINGS: usize = 500;

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupFile {
    pub version: u32,
    pub created_at: i64,
    pub app_version: String,
    pub settings: Vec<(String, String)>,
    pub mappings: Vec<Mapping>,
    pub tasks: Vec<TaskSchedule>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskSchedule {
    pub name: String,
    pub enabled: bool,
    pub interval_secs: i64,
}

#[derive(Debug, Serialize)]
pub struct BackupInfo {
    pub name: String,
    pub bytes: u64,
    pub modified_at: i64,
}

/// Only names this module generates are ever read or served -- keeps the
/// download endpoint from being a path-traversal primitive.
pub fn is_valid_name(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix("settings-")
        .and_then(|r| r.strip_suffix(".json"))
    else {
        return false;
    };
    let Some((date, time)) = rest.split_once('-') else {
        return false;
    };
    date.len() == 8
        && time.len() == 6
        && date.bytes().all(|b| b.is_ascii_digit())
        && time.bytes().all(|b| b.is_ascii_digit())
}

fn file_name(ts: i64) -> String {
    let (y, mo, d, h, mi, s) = civil(ts);
    format!("settings-{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}.json")
}

/// Unix seconds -> UTC (year, month, day, hour, minute, second).
fn civil(ts: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, secs / 3600, secs % 3600 / 60, secs % 60)
}

pub fn collect(conn: &Connection, now: i64, app_version: &str) -> Result<BackupFile> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
    let settings = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<(String, String)>>>()?;
    let tasks = db::list_tasks(conn)?
        .into_iter()
        .map(|t| TaskSchedule {
            name: t.name,
            enabled: t.enabled,
            interval_secs: t.interval_secs,
        })
        .collect();
    Ok(BackupFile {
        version: FORMAT_VERSION,
        created_at: now,
        app_version: app_version.to_string(),
        settings,
        mappings: db::list_mappings(conn)?,
        tasks,
    })
}

/// Write a backup into `dir` (created if needed) and prune to `KEEP`.
/// Returns the new file's name. Written to a temp name then renamed, so a
/// crash can't leave a truncated file that looks like a real backup.
pub fn create(conn: &Connection, dir: &Path, now: i64, app_version: &str) -> Result<String> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let body = serde_json::to_vec_pretty(&collect(conn, now, app_version)?)?;
    let mut name = file_name(now);
    // Two backups in the same second (manual + scheduled) must not overwrite.
    let mut n = 1;
    while dir.join(&name).exists() {
        name = file_name(now + n);
        n += 1;
    }
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, dir.join(&name))?;
    prune(dir, KEEP);
    Ok(name)
}

/// Newest first.
pub fn list(dir: &Path) -> Vec<BackupInfo> {
    let mut out: Vec<BackupInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !is_valid_name(&name) {
                return None;
            }
            let meta = e.metadata().ok()?;
            Some(BackupInfo {
                bytes: meta.len(),
                modified_at: meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs() as i64),
                name,
            })
        })
        .collect();
    // Names embed the timestamp, so they sort chronologically.
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

fn prune(dir: &Path, keep: usize) {
    for old in list(dir).into_iter().skip(keep) {
        let _ = std::fs::remove_file(dir.join(old.name));
    }
}

pub fn parse(bytes: &[u8]) -> Result<BackupFile> {
    let file: BackupFile = serde_json::from_slice(bytes).context("not a valid backup file")?;
    if file.version != FORMAT_VERSION {
        bail!(
            "backup format version {} isn't supported (this app reads version {FORMAT_VERSION})",
            file.version
        );
    }
    if file.settings.len() > MAX_SETTINGS {
        bail!("backup has too many settings");
    }
    if file.settings.iter().any(|(k, _)| k.trim().is_empty()) {
        bail!("backup contains an empty setting name");
    }
    Ok(file)
}

/// Replace the app's configuration with the backup's, all-or-nothing.
pub fn restore(conn: &mut Connection, file: &BackupFile) -> Result<()> {
    {
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM settings", [])?;
        for (k, v) in &file.settings {
            tx.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2)",
                params![k, v],
            )?;
        }
        for t in &file.tasks {
            // Unknown task names (from a newer version) are ignored.
            tx.execute(
                "UPDATE scheduled_task SET enabled = ?2, interval_secs = ?3 WHERE name = ?1",
                params![t.name, t.enabled, t.interval_secs],
            )?;
        }
        tx.commit()?;
    }
    db::replace_mappings(conn, &file.mappings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_matches_known_timestamps() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil(1_792_899_458), (2026, 10, 25, 3, 37, 38));
        assert_eq!(file_name(951_782_400), "settings-20000229-000000.json"); // leap day
    }

    #[test]
    fn only_generated_names_are_valid() {
        assert!(is_valid_name("settings-20261025-041738.json"));
        for bad in [
            "../settings-20261025-041738.json",
            "settings-20261025-041738.json.tmp",
            "settings-2026102a-041738.json",
            "settings-20261025-04173.json",
            "settings-20261025-041738/../x.json",
            "index.sqlite3",
            "",
        ] {
            assert!(!is_valid_name(bad), "{bad}");
        }
    }

    fn seeded() -> r2d2::Pool<r2d2_sqlite::SqliteConnectionManager> {
        let pool = db::test_pool();
        let mut conn = pool.get().unwrap();
        db::set_setting(&conn, "plex_url", "http://plex:32400").unwrap();
        db::set_setting(&conn, "plex_token", "secret").unwrap();
        db::replace_mappings(
            &mut conn,
            &[Mapping {
                plex_prefix: "/tv".into(),
                container_path: "/media/tv".into(),
                label: None,
            }],
        )
        .unwrap();
        db::ensure_task(&conn, "library_scan", true, 86_400, None).unwrap();
        pool
    }

    #[test]
    fn a_backup_restores_settings_mappings_and_schedules_after_changes() {
        let pool = seeded();
        let dir = tempfile::tempdir().unwrap();
        let name = create(&pool.get().unwrap(), dir.path(), 1_792_899_458, "v1").unwrap();

        let mut conn = pool.get().unwrap();
        db::set_setting(&conn, "plex_url", "http://wrong").unwrap();
        db::set_setting(&conn, "extra", "x").unwrap();
        db::replace_mappings(&mut conn, &[]).unwrap();
        db::set_task_schedule(&conn, "library_scan", false, 3_600).unwrap();

        let file = parse(&std::fs::read(dir.path().join(name)).unwrap()).unwrap();
        restore(&mut conn, &file).unwrap();

        assert_eq!(
            db::get_setting(&conn, "plex_url").unwrap().unwrap(),
            "http://plex:32400"
        );
        assert_eq!(db::get_setting(&conn, "extra").unwrap(), None);
        assert_eq!(db::list_mappings(&conn).unwrap().len(), 1);
        let t = db::list_tasks(&conn).unwrap().pop().unwrap();
        assert!(t.enabled);
        assert_eq!(t.interval_secs, 86_400);
    }

    #[test]
    fn same_second_backups_dont_overwrite_and_old_ones_are_pruned() {
        let pool = seeded();
        let conn = pool.get().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let a = create(&conn, dir.path(), 1_000_000, "v").unwrap();
        let b = create(&conn, dir.path(), 1_000_000, "v").unwrap();
        assert_ne!(a, b);
        for i in 0..(KEEP as i64 + 3) {
            create(&conn, dir.path(), 2_000_000 + i * 100, "v").unwrap();
        }
        let names: Vec<_> = list(dir.path()).into_iter().map(|b| b.name).collect();
        assert_eq!(names.len(), KEEP);
        assert!(!names.contains(&a), "oldest should have been pruned");
        assert!(names[0] > names[1], "newest first");
    }

    #[test]
    fn bad_backups_are_rejected_without_touching_anything() {
        assert!(parse(b"not json").is_err());
        let wrong = br#"{"version":99,"created_at":0,"app_version":"","settings":[],"mappings":[],"tasks":[]}"#;
        assert!(parse(wrong).unwrap_err().to_string().contains("version 99"));
        let empty_key = br#"{"version":1,"created_at":0,"app_version":"","settings":[[" ","x"]],"mappings":[],"tasks":[]}"#;
        assert!(parse(empty_key).is_err());
    }
}
