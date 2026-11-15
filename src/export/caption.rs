//! What it takes to draw a caption: where the subtitle lines come from, how the
//! text looks, and the files ffmpeg reads them from while it runs.
//!
//! These small types stand in for the long argument lists the filter and export
//! builders used to take (the caption choice, the font, the three temp files...).

use super::*;

/// Which stretch of the media a caption is for, and the subtitle lines to draw
/// in it (when the caption burns subtitles and carries none of its own).
#[derive(Clone, Copy)]
pub struct CaptionWindow<'a> {
    pub cues: &'a [Cue],
    pub start_ms: i64,
    pub end_ms: i64,
    /// Sync correction: positive moves lines later, negative earlier.
    pub offset_ms: i64,
}

/// How a caption looks: the choice itself, the font file it is drawn in, and
/// the picture height that meme text is placed against.
#[derive(Clone, Copy)]
pub struct CaptionStyle<'a> {
    pub caption: Option<&'a Caption>,
    pub font_path: &'a str,
    pub height_hint: u32,
}

impl<'a> CaptionStyle<'a> {
    pub fn new(caption: Option<&'a Caption>, font_path: &'a str, height_hint: u32) -> Self {
        CaptionStyle {
            caption,
            font_path,
            height_hint,
        }
    }

    /// No caption at all (the font is not used).
    pub fn none(height_hint: u32) -> Self {
        CaptionStyle::new(None, "", height_hint)
    }
}

/// A caption's style together with the temp files it reads, as they are named
/// in a filter chain.
#[derive(Clone, Copy)]
pub struct CaptionSpec<'a> {
    pub style: CaptionStyle<'a>,
    pub srt: Option<&'a str>,
    pub top: Option<&'a str>,
    pub bottom: Option<&'a str>,
}

impl<'a> CaptionSpec<'a> {
    /// A caption whose files, if any, are the given paths.
    pub fn new(
        style: CaptionStyle<'a>,
        srt: Option<&'a str>,
        top: Option<&'a str>,
        bottom: Option<&'a str>,
    ) -> Self {
        CaptionSpec {
            style,
            srt,
            top,
            bottom,
        }
    }

    /// No caption (so no files).
    pub fn none(height_hint: u32) -> Self {
        CaptionSpec::new(CaptionStyle::none(height_hint), None, None, None)
    }
}

/// The caption files written for one render. They are removed when this is
/// dropped (see [`CaptionFiles`]), so a render that is cancelled part way
/// doesn't leave them behind.
pub struct WrittenCaptions {
    pub srt: Option<String>,
    pub top: Option<String>,
    pub bottom: Option<String>,
    pub files: CaptionFiles,
}

impl WrittenCaptions {
    /// The spec to build filters from, borrowing these files' paths.
    pub fn spec<'a>(&'a self, style: CaptionStyle<'a>) -> CaptionSpec<'a> {
        CaptionSpec::new(
            style,
            self.srt.as_deref(),
            self.top.as_deref(),
            self.bottom.as_deref(),
        )
    }

    pub fn cleanup(&self) {
        self.files.cleanup();
    }
}

/// Write whatever caption files a caption mode needs (SRT for source subs,
/// text files for meme top/bottom lines) to `tmp_dir`, named from `id`.
pub fn write_caption_files(
    tmp_dir: &Path,
    id: &str,
    caption: Option<&Caption>,
    window: &CaptionWindow,
) -> Result<WrittenCaptions> {
    let mut written = WrittenCaptions {
        srt: None,
        top: None,
        bottom: None,
        files: CaptionFiles {
            srt: None,
            text_files: Vec::new(),
        },
    };

    match caption {
        Some(Caption::Source {
            cues: override_cues,
            ..
        }) => {
            // An edited line list (corrected/shortened/reworded in the
            // editor) replaces the indexed cues entirely; either way the
            // offset still applies uniformly below.
            let effective: &[Cue] = override_cues.as_deref().unwrap_or(window.cues);
            let srt =
                subtitles::trimmed_srt(effective, window.start_ms, window.end_ms, window.offset_ms);
            // No line falls inside the window: there is nothing to burn. An empty
            // SRT would make ffmpeg's subtitles filter fail ("Unable to open ...
            // Invalid data found"), so no file is written and no filter is built.
            if !srt.trim().is_empty() {
                let path = tmp_dir.join(format!("{id}.srt"));
                std::fs::write(&path, srt)?;
                written.srt = Some(path.to_string_lossy().to_string());
                written.files.srt = Some(path);
            }
        }
        Some(Caption::Meme { top, bottom, .. }) => {
            if let Some(t) = top.as_ref().filter(|s| !s.trim().is_empty()) {
                let path = tmp_dir.join(format!("{id}.top.txt"));
                std::fs::write(&path, t)?;
                written.top = Some(path.to_string_lossy().to_string());
                written.files.text_files.push(path);
            }
            if let Some(b) = bottom.as_ref().filter(|s| !s.trim().is_empty()) {
                let path = tmp_dir.join(format!("{id}.bottom.txt"));
                std::fs::write(&path, b)?;
                written.bottom = Some(path.to_string_lossy().to_string());
                written.files.text_files.push(path);
            }
        }
        _ => {}
    }

    Ok(written)
}
