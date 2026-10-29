//! Running ffmpeg: single-clip and sequence exports, caption temp files, GIF optimisation.

use super::*;

/// Write whatever caption files a caption mode needs (SRT for source subs,
/// text files for meme top/bottom lines) to `tmp_dir`, named from `id`.
/// Returns the paths to pass into a chain-builder, plus the files to clean up.
#[allow(clippy::too_many_arguments)]
pub(super) fn write_caption_files(
    tmp_dir: &Path,
    id: &str,
    caption: Option<&Caption>,
    cues: &[Cue],
    start_ms: i64,
    end_ms: i64,
    offset_ms: i64,
) -> Result<CaptionFilePaths> {
    let mut files = CaptionFiles {
        srt: None,
        text_files: Vec::new(),
    };
    let mut srt_path: Option<String> = None;
    let mut top_file: Option<String> = None;
    let mut bottom_file: Option<String> = None;

    match caption {
        Some(Caption::Source {
            cues: override_cues,
        }) => {
            // An edited line list (corrected/shortened/reworded in the
            // editor) replaces the indexed cues entirely; either way the
            // offset still applies uniformly below.
            let effective: &[Cue] = override_cues.as_ref().map(|v| v.as_slice()).unwrap_or(cues);
            let srt = subtitles::trimmed_srt(effective, start_ms, end_ms, offset_ms);
            let path = tmp_dir.join(format!("{id}.srt"));
            std::fs::write(&path, srt)?;
            srt_path = Some(path.to_string_lossy().to_string());
            files.srt = Some(path);
        }
        Some(Caption::Meme { top, bottom, .. }) => {
            if let Some(t) = top.as_ref().filter(|s| !s.trim().is_empty()) {
                let path = tmp_dir.join(format!("{id}.top.txt"));
                std::fs::write(&path, t)?;
                top_file = Some(path.to_string_lossy().to_string());
                files.text_files.push(path);
            }
            if let Some(b) = bottom.as_ref().filter(|s| !s.trim().is_empty()) {
                let path = tmp_dir.join(format!("{id}.bottom.txt"));
                std::fs::write(&path, b)?;
                bottom_file = Some(path.to_string_lossy().to_string());
                files.text_files.push(path);
            }
        }
        _ => {}
    }

    Ok((srt_path, top_file, bottom_file, files))
}

/// Write temp caption files (srt for source subs, text files for meme lines) and
/// return both the files to clean up and the core filter chain to use.
#[allow(clippy::too_many_arguments)]
pub fn prepare_captions(
    tmp_dir: &Path,
    id: &str,
    width: u32,
    fps: u32,
    caption: Option<&Caption>,
    cues: &[Cue],
    start_ms: i64,
    end_ms: i64,
    subtitle_offset_ms: i64,
    font_path: &str,
    frame_height_hint: u32,
) -> Result<(String, CaptionFiles)> {
    let (srt_path, top_file, bottom_file, files) = write_caption_files(
        tmp_dir,
        id,
        caption,
        cues,
        start_ms,
        end_ms,
        subtitle_offset_ms,
    )?;
    let core = core_chain(
        width,
        fps,
        caption,
        font_path,
        srt_path.as_deref(),
        top_file.as_deref(),
        bottom_file.as_deref(),
        frame_height_hint,
    );
    Ok((core, files))
}

/// Run a full export end to end, writing the finished file to `out_path`.
#[allow(clippy::too_many_arguments)]
pub async fn run_export(
    ffmpeg_bin: &str,
    gifsicle_bin: &str,
    tmp_dir: &Path,
    job_id: &str,
    file: &Path,
    req: &ExportRequest,
    cues: &[Cue],
    font_path: &str,
    frame_height_hint: u32,
    source_title: Option<&str>,
    out_path: &Path,
) -> Result<()> {
    std::fs::create_dir_all(tmp_dir)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let (core, caption_files) = prepare_captions(
        tmp_dir,
        job_id,
        req.width,
        req.fps,
        req.caption.as_ref(),
        cues,
        req.start_ms,
        req.end_ms,
        req.subtitle_offset_ms,
        font_path,
        frame_height_hint,
    )?;
    let vf = video_filter(req.format, &core);
    let audio_mode = if req.audio {
        AudioMode::Encode
    } else {
        AudioMode::None
    };
    let codec = codec_args(req.format, req.quality, audio_mode);
    let args = export_args(
        &file.to_string_lossy(),
        req.start_ms,
        req.end_ms,
        &vf,
        &codec,
        source_title.filter(|_| req.credit_source),
        &out_path.to_string_lossy(),
    );
    let result = ffmpeg::run(ffmpeg_bin, &args).await;
    caption_files.cleanup();
    result?;
    maybe_optimize_gif(gifsicle_bin, req.format, req.quality, out_path).await;
    Ok(())
}

/// GIF-only, best-effort post-process: a failure here never fails the
/// export, since ffmpeg already wrote a valid, playable GIF — this is purely
/// a size optimization on top of it. Logged loudly on failure so it's still
/// diagnosable, just not fatal.
pub(super) async fn maybe_optimize_gif(
    gifsicle_bin: &str,
    format: Format,
    quality: u32,
    out_path: &Path,
) {
    if format != Format::Gif {
        return;
    }
    if let Err(e) = ffmpeg::gifsicle_optimize(gifsicle_bin, out_path, quality).await {
        warn!(
            "gifsicle optimization skipped for {}: {e:#}",
            out_path.display()
        );
    }
}

/// A resolved, ready-to-render clip for a sequence export: its source file on
/// disk, its own in/out window, its own caption choice, and the subtitle cues
/// (already fetched from that clip's own media) a "source" caption would burn.
pub struct ResolvedSequenceClip {
    pub file: PathBuf,
    pub start_ms: i64,
    pub end_ms: i64,
    pub caption: Option<Caption>,
    pub subtitle_offset_ms: i64,
    pub cues: Vec<Cue>,
    /// What this clip came from (see `source_label`), for the sequence's title tag.
    pub source: String,
}

/// Render one sequence clip to an intermediate file matched to the shared
/// canvas size (letterboxed to preserve its own aspect ratio), with its own
/// caption burned in. Decent fixed quality — this is an intermediate; the
/// sequence's chosen quality is applied once, in the final encode pass.
///
/// When `audio` is set, the part carries uncompressed PCM audio; phase 3
/// does the one lossy Opus encode over the joined result.
#[allow(clippy::too_many_arguments)]
pub(super) async fn render_sequence_clip(
    ffmpeg_bin: &str,
    ffprobe_bin: &str,
    tmp_dir: &Path,
    id: &str,
    clip: &ResolvedSequenceClip,
    fps: u32,
    canvas: (u32, u32),
    font_path: &str,
    audio: bool,
) -> Result<PathBuf> {
    let (width, height) = canvas;
    let (srt_path, top_file, bottom_file, caption_files) = write_caption_files(
        tmp_dir,
        id,
        clip.caption.as_ref(),
        &clip.cues,
        clip.start_ms,
        clip.end_ms,
        clip.subtitle_offset_ms,
    )?;
    let core = clip_core_chain(
        width,
        height,
        fps,
        clip.caption.as_ref(),
        font_path,
        srt_path.as_deref(),
        top_file.as_deref(),
        bottom_file.as_deref(),
        height,
    );
    let out_path = tmp_dir.join(format!("{id}.part.{}", part_ext(audio)));

    let args = if audio {
        // A probe failure just means "treat this clip as silent" rather
        // than failing the whole sequence over it -- the clip still
        // exports fine, just without sound for this one segment.
        let has_audio = ffmpeg::probe(ffprobe_bin, &clip.file)
            .await
            .map(|p| p.has_audio)
            .unwrap_or(false);
        sequence_clip_args_with_audio(
            &clip.file,
            clip.start_ms,
            clip.end_ms,
            &core,
            has_audio,
            &out_path,
        )
    } else {
        let codec = codec_args(Format::Mp4, 90, AudioMode::None);
        export_args(
            &clip.file.to_string_lossy(),
            clip.start_ms,
            clip.end_ms,
            &core,
            &codec,
            None, // an intermediate: no title, and none of the source's tags
            &out_path.to_string_lossy(),
        )
    };

    let result = ffmpeg::run(ffmpeg_bin, &args).await;
    caption_files.cleanup();
    result?;
    Ok(out_path)
}

/// Intermediates with PCM audio need a container that holds it (MP4 does not
/// do so reliably); without audio the plain MP4 will do.
fn part_ext(audio: bool) -> &'static str {
    if audio {
        "mov"
    } else {
        "mp4"
    }
}

/// Build the ffmpeg argv for one sequence clip's audio-bearing intermediate.
/// Always produces exactly one video + one PCM audio stream, even when the
/// source clip has no audio track of its own (silence is substituted) --
/// the concat demuxer downstream requires every segment to have matching
/// streams, so a clip that silently lacked an audio track would otherwise
/// corrupt or undefine the concatenated result.
pub(super) fn sequence_clip_args_with_audio(
    file: &Path,
    start_ms: i64,
    end_ms: i64,
    vf: &str,
    has_audio: bool,
    out: &Path,
) -> Vec<String> {
    let dur = (end_ms - start_ms).max(0);
    let mut args: Vec<String> = vec!["-y".into(), "-v".into(), "error".into()];
    // Real input, trimmed via fast seek (input 0).
    args.extend([
        "-ss".into(),
        ms_to_secs(start_ms),
        "-t".into(),
        ms_to_secs(dur),
        "-i".into(),
        file.to_string_lossy().into_owned(),
    ]);
    if has_audio {
        args.extend([
            "-vf".into(),
            vf.into(),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "0:a:0".into(),
        ]);
    } else {
        // A second, infinite silent input (input 1) -- `-shortest` below
        // caps it to the real video's duration.
        args.extend([
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "anullsrc=channel_layout=stereo:sample_rate=48000".into(),
            "-vf".into(),
            vf.into(),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "1:a:0".into(),
            "-shortest".into(),
        ]);
    }
    args.extend(codec_args(Format::Mp4, 90, AudioMode::Pcm));
    // An intermediate, but it is cut from the source file, so nothing of the
    // source's tags may ride along into the finished sequence.
    args.extend(privacy_args(None));
    args.push(out.to_string_lossy().into_owned());
    args
}

/// Build an ffmpeg concat-demuxer list file's contents from a set of part
/// paths. Each path is single-quoted per the demuxer's syntax, with embedded
/// single quotes escaped (`'` -> `'\''`), since paths can contain almost
/// anything (titles, episode names) once they reach the temp directory.
pub(super) fn concat_list(parts: &[PathBuf]) -> String {
    parts
        .iter()
        .map(|p| format!("file '{}'", p.to_string_lossy().replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render N clips — possibly from different source files — back to back into
/// one output. Each clip is first rendered to a common canvas (phase 1), all
/// parts are concatenated into a single intermediate (phase 2), then that
/// combined video is run through the normal single-pass format encoder —
/// palette generation for GIF, transcode for the others (phase 3). Captions
/// are burned per clip in phase 1, so phase 3 never applies a caption.
#[allow(clippy::too_many_arguments)]
pub async fn run_sequence_export(
    ffmpeg_bin: &str,
    gifsicle_bin: &str,
    ffprobe_bin: &str,
    tmp_dir: &Path,
    job_id: &str,
    clips: &[ResolvedSequenceClip],
    format: Format,
    width: u32,
    fps: u32,
    quality: u32,
    audio: bool,
    font_path: &str,
    source_title: Option<&str>,
    out_path: &Path,
) -> Result<()> {
    std::fs::create_dir_all(tmp_dir)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let canvas = canvas_dims(width);

    // Phase 1: render each clip to a canvas-matched intermediate.
    let mut parts = Vec::with_capacity(clips.len());
    for (i, clip) in clips.iter().enumerate() {
        let id = format!("{job_id}-{i}");
        match render_sequence_clip(
            ffmpeg_bin,
            ffprobe_bin,
            tmp_dir,
            &id,
            clip,
            fps,
            canvas,
            font_path,
            audio,
        )
        .await
        {
            Ok(path) => parts.push(path),
            Err(e) => {
                cleanup_paths(&parts);
                return Err(e);
            }
        }
    }

    // Phase 2: concatenate the parts into one intermediate (video re-encoded,
    // not stream-copied, so we're not relying on every part's container
    // metadata lining up exactly; audio -- when present -- is a straight
    // stream copy, valid because phase 1 gave every part the same PCM params).
    let list_path = tmp_dir.join(format!("{job_id}-list.txt"));
    if let Err(e) = std::fs::write(&list_path, concat_list(&parts)) {
        cleanup_paths(&parts);
        return Err(e.into());
    }
    let concat_path = tmp_dir.join(format!("{job_id}-concat.{}", part_ext(audio)));
    let mut concat_args = vec![
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-f".into(),
        "concat".into(),
        "-safe".into(),
        "0".into(),
        "-i".into(),
        list_path.to_string_lossy().into_owned(),
        "-c:v".into(),
        "libx264".into(),
        "-crf".into(),
        "20".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
    ];
    concat_args.extend(audio_args(if audio {
        AudioMode::Copy // PCM from the parts, joined seamlessly
    } else {
        AudioMode::None
    }));
    concat_args.extend(privacy_args(None));
    concat_args.push(concat_path.to_string_lossy().into_owned());
    if let Err(e) = ffmpeg::run(ffmpeg_bin, &concat_args).await {
        cleanup_paths(&parts);
        let _ = std::fs::remove_file(&list_path);
        return Err(e);
    }
    cleanup_paths(&parts);
    let _ = std::fs::remove_file(&list_path);

    // Phase 3: encode the concatenated video to the requested format. The
    // concatenated file is already exactly `canvas` sized, so core_chain's
    // scale is a no-op here — it exists only to apply fps + (no) caption.
    // Audio -- when present -- is encoded to Opus here, once, over the
    // joined PCM.
    let probed = ffmpeg::probe(ffprobe_bin, &concat_path).await;
    let total_ms: i64 = match &probed {
        Ok(p) if p.duration_ms > 0 => p.duration_ms,
        _ => clips.iter().map(|c| c.end_ms - c.start_ms).sum(),
    };
    let core = core_chain(canvas.0, fps, None, font_path, None, None, None, canvas.1);
    let vf = video_filter(format, &core);
    let codec = codec_args(
        format,
        quality,
        if audio {
            AudioMode::Encode // the one lossy audio encode, over the joined audio
        } else {
            AudioMode::None
        },
    );
    let args = export_args(
        &concat_path.to_string_lossy(),
        0,
        total_ms,
        &vf,
        &codec,
        source_title,
        &out_path.to_string_lossy(),
    );
    let result = ffmpeg::run(ffmpeg_bin, &args).await;
    let _ = std::fs::remove_file(&concat_path);
    result?;
    maybe_optimize_gif(gifsicle_bin, format, quality, out_path).await;
    Ok(())
}

pub(super) fn cleanup_paths(paths: &[PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}
