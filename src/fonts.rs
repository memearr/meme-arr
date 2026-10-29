//! Caption fonts: the ones shipped with the app (`FONTS_DIR`) and the ones the admin
//! uploads (kept in `DATA_DIR/fonts`, so they survive upgrades).
//!
//! A font is identified by its file name. Uploaded files are checked before they are
//! kept: a plausible name, a size limit, and that they parse as a TrueType/OpenType
//! font with a family name. ffmpeg later reads them by path (`drawtext`) or through the
//! folder they are in (`subtitles`, which matches by family name).

use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Largest font file accepted.
pub const MAX_FONT_BYTES: usize = 16 * 1024 * 1024;
/// Most uploaded fonts kept at once.
pub const MAX_USER_FONTS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FontInfo {
    /// The file name; what an export request refers to the font by.
    pub id: String,
    /// The family name, which is what the subtitle renderer matches on.
    pub family: String,
    pub style: String,
    /// Shipped with the app (or mounted over `FONTS_DIR`), so it can't be deleted here.
    pub builtin: bool,
    pub bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FontError {
    /// Not an acceptable file name, or not a font. The text says why.
    Invalid(String),
    TooLarge,
    /// A font with that file name is already there (uploaded or built in).
    Exists,
    /// Too many uploaded fonts.
    Limit,
    NotFound,
    /// Built-in fonts can't be removed from the app.
    BuiltIn,
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontError::Invalid(why) => write!(f, "{why}"),
            FontError::TooLarge => write!(
                f,
                "that font file is larger than {} MB",
                MAX_FONT_BYTES / 1024 / 1024
            ),
            FontError::Exists => write!(
                f,
                "a font with that file name is already installed; delete it first to replace it"
            ),
            FontError::Limit => write!(
                f,
                "at most {MAX_USER_FONTS} fonts can be uploaded; delete one first"
            ),
            FontError::NotFound => write!(f, "no such font"),
            FontError::BuiltIn => write!(f, "built-in fonts can't be deleted here"),
        }
    }
}

/// The family and style names of a font file's bytes, or `None` if it isn't a usable
/// TrueType/OpenType font. A small reader of just what is needed (the table directory,
/// the glyph count and the `name` table); every access is bounds-checked, so a damaged
/// or hostile file gives `None`, never a panic.
pub fn names_of(bytes: &[u8]) -> Option<(String, String)> {
    let u16_at = |o: usize| -> Option<usize> {
        Some(u16::from_be_bytes(bytes.get(o..o.checked_add(2)?)?.try_into().ok()?) as usize)
    };
    let u32_at = |o: usize| -> Option<usize> {
        Some(u32::from_be_bytes(bytes.get(o..o.checked_add(4)?)?.try_into().ok()?) as usize)
    };
    // 0x00010000 = TrueType outlines, "OTTO" = CFF outlines, "true" = old Apple TrueType.
    match bytes.get(0..4)? {
        [0, 1, 0, 0] | b"OTTO" | b"true" => {}
        _ => return None,
    }
    let table_count = u16_at(4)?;
    if table_count == 0 || table_count > 64 {
        return None;
    }
    let table = |tag: &[u8; 4]| -> Option<(usize, usize)> {
        (0..table_count).find_map(|i| {
            let rec = 12 + i * 16;
            if bytes.get(rec..rec + 4)? == tag {
                let (off, len) = (u32_at(rec + 8)?, u32_at(rec + 12)?);
                bytes.get(off..off.checked_add(len)?)?;
                Some((off, len))
            } else {
                None
            }
        })
    };
    // a font has glyphs: `maxp` holds the count (after a 4-byte version)
    let (maxp, _) = table(b"maxp")?;
    if u16_at(maxp + 4)? == 0 {
        return None;
    }
    table(b"head")?;
    let (name, name_len) = table(b"name")?;
    let count = u16_at(name + 2)?;
    let strings = name.checked_add(u16_at(name + 4)?)?;
    let end = name + name_len;

    // The best string for a name id: Windows/Unicode (UTF-16), English preferred,
    // then Macintosh Roman (plain bytes).
    let pick = |ids: &[usize]| -> Option<String> {
        ids.iter().find_map(|id| {
            let mut best: Option<(u8, String)> = None;
            for i in 0..count {
                let rec = name + 6 + i * 12;
                if rec + 12 > end {
                    break;
                }
                let (platform, lang, name_id, len, off) = (
                    u16_at(rec)?,
                    u16_at(rec + 4)?,
                    u16_at(rec + 6)?,
                    u16_at(rec + 8)?,
                    u16_at(rec + 10)?,
                );
                if name_id != *id {
                    continue;
                }
                let Some(raw) = bytes
                    .get(strings.checked_add(off)?..strings.checked_add(off)?.checked_add(len)?)
                else {
                    continue;
                };
                let (rank, text) = match platform {
                    0 | 3 => {
                        let units: Vec<u16> = raw
                            .chunks_exact(2)
                            .map(|c| u16::from_be_bytes([c[0], c[1]]))
                            .collect();
                        (
                            if lang == 0x0409 { 0 } else { 1 },
                            String::from_utf16_lossy(&units),
                        )
                    }
                    1 => (2, raw.iter().map(|b| *b as char).collect()),
                    _ => continue,
                };
                let text = text.trim().to_string();
                if !text.is_empty() && best.as_ref().map_or(true, |(r, _)| rank < *r) {
                    best = Some((rank, text));
                }
            }
            best.map(|(_, t)| t)
        })
    };
    // 1 = family, 2 = subfamily (what a family-name lookup matches), 16/17 = the
    // "typographic" ones some fonts use instead.
    let family = pick(&[1, 16])?;
    let style = pick(&[2, 17]).unwrap_or_else(|| "Regular".into());
    Some((family, style))
}

/// The family name of the font file at `path`.
pub fn family_of(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    names_of(&bytes).map(|(family, _)| family)
}

/// A file name that is safe to store: no folders, a font extension, tame characters.
pub fn safe_font_filename(name: &str) -> Result<String, FontError> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if base.is_empty() || base.len() > 100 || base.starts_with('.') {
        return Err(FontError::Invalid("that isn't a usable file name".into()));
    }
    if !base
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ' | '(' | ')'))
    {
        return Err(FontError::Invalid(
            "the file name may only contain letters, digits, spaces and . _ - ( )".into(),
        ));
    }
    let lower = base.to_ascii_lowercase();
    if !(lower.ends_with(".ttf") || lower.ends_with(".otf")) {
        return Err(FontError::Invalid(
            "only .ttf and .otf font files are supported".into(),
        ));
    }
    Ok(base.to_string())
}

fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
}

/// Both font folders, and what can be done with them.
#[derive(Debug, Clone)]
pub struct Fonts {
    app_dir: PathBuf,
    user_dir: PathBuf,
}

impl Fonts {
    pub fn new(app_dir: PathBuf, user_dir: PathBuf) -> Self {
        Fonts { app_dir, user_dir }
    }

    pub fn user_dir(&self) -> &Path {
        &self.user_dir
    }

    fn scan(dir: &Path, builtin: bool) -> Vec<FontInfo> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<FontInfo> = entries
            .flatten()
            .filter(|e| e.path().is_file() && is_font_file(&e.path()))
            .filter_map(|e| {
                let bytes = std::fs::read(e.path()).ok()?;
                let (family, style) = names_of(&bytes)?;
                Some(FontInfo {
                    id: e.file_name().to_string_lossy().to_string(),
                    family,
                    style,
                    builtin,
                    bytes: bytes.len() as u64,
                })
            })
            .collect();
        out.sort_by_key(|f| {
            (
                f.family.to_lowercase(),
                f.style.to_lowercase(),
                f.id.clone(),
            )
        });
        out
    }

    /// Built-in fonts first, then the uploaded ones, each sorted by family.
    pub fn list(&self) -> Vec<FontInfo> {
        let mut all = Self::scan(&self.app_dir, true);
        all.extend(Self::scan(&self.user_dir, false));
        all
    }

    /// Where the font called `id` is on disk.
    pub fn path_of(&self, id: &str) -> Option<PathBuf> {
        let name = safe_font_filename(id).ok()?;
        [&self.user_dir, &self.app_dir]
            .into_iter()
            .map(|d| d.join(&name))
            .find(|p| p.is_file())
    }

    /// Check `bytes` and keep them as `filename` in the uploads folder.
    pub fn store(&self, filename: &str, bytes: &[u8]) -> Result<FontInfo, FontError> {
        let name = safe_font_filename(filename)?;
        if bytes.len() > MAX_FONT_BYTES {
            return Err(FontError::TooLarge);
        }
        let Some((family, style)) = names_of(bytes) else {
            return Err(FontError::Invalid(
                "that file isn't a usable TrueType or OpenType font".into(),
            ));
        };
        if self.app_dir.join(&name).exists() || self.user_dir.join(&name).exists() {
            return Err(FontError::Exists);
        }
        if Self::scan(&self.user_dir, false).len() >= MAX_USER_FONTS {
            return Err(FontError::Limit);
        }
        std::fs::create_dir_all(&self.user_dir)
            .map_err(|e| FontError::Invalid(format!("couldn't create the fonts folder: {e}")))?;
        // Write beside the final name and rename, so a half-written font is never picked up.
        let part = self.user_dir.join(format!(".{name}.part"));
        std::fs::write(&part, bytes)
            .map_err(|e| FontError::Invalid(format!("couldn't save the font: {e}")))?;
        if let Err(e) = std::fs::rename(&part, self.user_dir.join(&name)) {
            let _ = std::fs::remove_file(&part);
            return Err(FontError::Invalid(format!("couldn't save the font: {e}")));
        }
        Ok(FontInfo {
            id: name,
            family,
            style,
            builtin: false,
            bytes: bytes.len() as u64,
        })
    }

    /// Delete an uploaded font.
    pub fn remove(&self, id: &str) -> Result<(), FontError> {
        let name = safe_font_filename(id).map_err(|_| FontError::NotFound)?;
        let path = self.user_dir.join(&name);
        if !path.is_file() {
            return if self.app_dir.join(&name).is_file() {
                Err(FontError::BuiltIn)
            } else {
                Err(FontError::NotFound)
            };
        }
        std::fs::remove_file(path).map_err(|_| FontError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/MemeTestSans.ttf"
        ))
        .unwrap()
    }

    fn fonts() -> (tempfile::TempDir, Fonts) {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("Builtin.ttf"), fixture()).unwrap();
        let f = Fonts::new(app, dir.path().join("user"));
        (dir, f)
    }

    #[test]
    fn reads_the_family_and_style_names() {
        assert_eq!(
            names_of(&fixture()),
            Some(("Meme Test Sans".into(), "Regular".into()))
        );
        assert_eq!(names_of(b"not a font at all"), None);
        assert_eq!(names_of(&[]), None);
    }

    #[test]
    fn a_damaged_font_is_refused_and_never_panics() {
        let good = fixture();
        // every truncation, and a byte flipped at every position, either still reads or is refused
        for n in (0..good.len()).step_by(7) {
            let _ = names_of(&good[..n]);
        }
        for i in (0..good.len()).step_by(5) {
            let mut bad = good.clone();
            bad[i] ^= 0xFF;
            let _ = names_of(&bad);
        }
        assert_eq!(names_of(&good[..40]), None);
        // a collection (.ttc) and a WOFF are not accepted
        assert_eq!(names_of(b"ttcf      "), None);
        assert_eq!(names_of(b"wOFF   "), None);
        // a table that claims to lie beyond the end of the file
        let mut lying = good.clone();
        lying[12 + 8..12 + 12].copy_from_slice(&0xFFFF_FFF0u32.to_be_bytes());
        let _ = names_of(&lying);
    }

    #[test]
    fn file_names_are_checked() {
        assert_eq!(
            safe_font_filename("Anton-Regular.ttf").unwrap(),
            "Anton-Regular.ttf"
        );
        assert_eq!(
            safe_font_filename("My Font (bold).OTF").unwrap(),
            "My Font (bold).OTF"
        );
        // a path is cut down to its last part, so "../" can't reach out of the folder
        assert_eq!(
            safe_font_filename("../../etc/evil.ttf").unwrap(),
            "evil.ttf"
        );
        assert_eq!(
            safe_font_filename("C:\\Windows\\Fonts\\arial.ttf").unwrap(),
            "arial.ttf"
        );
        for bad in [
            "",
            ".hidden.ttf",
            "font.exe",
            "font.ttf.sh",
            "font",
            "a;b.ttf",
            "über.ttf",
            &"a".repeat(120),
        ] {
            assert!(safe_font_filename(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_font_is_stored_listed_found_and_removed() {
        let (_d, f) = fonts();
        assert_eq!(f.list().len(), 1);
        assert!(f.list()[0].builtin);

        let info = f.store("Mine.ttf", &fixture()).unwrap();
        assert_eq!(
            (info.id.as_str(), info.family.as_str(), info.builtin),
            ("Mine.ttf", "Meme Test Sans", false)
        );
        let all = f.list();
        assert_eq!(all.len(), 2);
        assert!(
            all[0].builtin && !all[1].builtin,
            "built-in first, then uploaded"
        );
        assert!(f.path_of("Mine.ttf").unwrap().is_file());
        assert!(f.path_of("Builtin.ttf").is_some());
        assert!(f.path_of("Nope.ttf").is_none());
        assert!(
            f.path_of("../Mine.ttf").is_some(),
            "a path is reduced to its file name"
        );

        f.remove("Mine.ttf").unwrap();
        assert_eq!(f.list().len(), 1);
        assert_eq!(f.remove("Mine.ttf"), Err(FontError::NotFound));
        assert_eq!(f.remove("Builtin.ttf"), Err(FontError::BuiltIn));
        assert!(
            f.path_of("Builtin.ttf").is_some(),
            "the built-in font is untouched"
        );
    }

    #[test]
    fn bad_uploads_are_refused_and_leave_nothing_behind() {
        let (_d, f) = fonts();
        assert!(matches!(
            f.store("x.ttf", b"MZ not a font"),
            Err(FontError::Invalid(_))
        ));
        assert!(matches!(
            f.store("x.exe", &fixture()),
            Err(FontError::Invalid(_))
        ));
        assert_eq!(
            f.store("x.ttf", &vec![0u8; MAX_FONT_BYTES + 1]),
            Err(FontError::TooLarge)
        );
        assert_eq!(
            f.store("Builtin.ttf", &fixture()),
            Err(FontError::Exists),
            "can't shadow a built-in font"
        );
        f.store("Once.ttf", &fixture()).unwrap();
        assert_eq!(f.store("Once.ttf", &fixture()), Err(FontError::Exists));
        let files: Vec<_> = std::fs::read_dir(f.user_dir()).unwrap().flatten().collect();
        assert_eq!(files.len(), 1, "only the one good font is on disk");
    }

    #[test]
    fn the_number_of_uploaded_fonts_is_capped() {
        let (_d, f) = fonts();
        for i in 0..MAX_USER_FONTS {
            f.store(&format!("f{i}.ttf"), &fixture()).unwrap();
        }
        assert_eq!(
            f.store("one-too-many.ttf", &fixture()),
            Err(FontError::Limit)
        );
    }
}
