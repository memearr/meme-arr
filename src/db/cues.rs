//! Subtitle cues: storing them for an item, and reading the ones in a time window.

use super::*;

pub fn insert_cues(
    conn: &mut rusqlite::Connection,
    media_id: i64,
    cues: &[crate::subtitles::Cue],
) -> Result<usize> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO subtitle_cue (media_id, start_ms, end_ms, text) VALUES (?1,?2,?3,?4)",
        )?;
        for cue in cues {
            stmt.execute(params![media_id, cue.start_ms, cue.end_ms, cue.text])?;
        }
    }
    tx.execute(
        "UPDATE media SET cue_count = cue_count + ?1 WHERE id = ?2",
        params![cues.len() as i64, media_id],
    )?;
    tx.commit()?;
    Ok(cues.len())
}

pub fn cues_in_window(
    conn: &rusqlite::Connection,
    media_id: i64,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<crate::subtitles::Cue>> {
    let mut stmt = conn.prepare(
        "SELECT start_ms, end_ms, text FROM subtitle_cue
         WHERE media_id = ?1 AND end_ms > ?2 AND start_ms < ?3
         ORDER BY start_ms",
    )?;
    let rows = stmt.query_map(params![media_id, start_ms, end_ms], |r| {
        Ok(crate::subtitles::Cue {
            start_ms: r.get(0)?,
            end_ms: r.get(1)?,
            text: r.get(2)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}
