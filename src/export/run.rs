//! Running ffmpeg: single-clip and sequence exports, caption temp files, GIF optimisation.

use super::*;

/// The programs the exports run.
#[derive(Clone, Copy)]
pub struct Tools<'a> {
    pub ffmpeg: &'a str,
    pub ffprobe: &'a str,
    pub gifsicle: &'a str,
}

/// Where one export works and writes: a name for its temp files, the folder they
/// go in, the finished file, and the one tag the file may carry (what it came from).
#[derive(Clone, Copy)]
pub struct JobSpec<'a> {
    pub id: &'a str,
    pub tmp_dir: &'a Path,
    pub out_path: &'a Path,
    pub source_title: Option<&'a str>,
}

impl JobSpec<'_> {
    /// Make sure the folder the finished file goes in, and the temp folder, exist.
    pub(super) fn prepare_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(self.tmp_dir)?;
        if let Some(parent) = self.out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

/// The picture height meme text is placed against, for a 16:9 picture `width` wide.
pub fn height_hint_for(width: u32) -> u32 {
    (width as f32 * 0.5625) as u32
}

/// Write temp caption files (srt for source subs, text files for meme lines) and
/// return both the files to clean up and the core filter chain to use.
pub fn prepare_captions(
    tmp_dir: &Path,
    id: &str,
    (width, fps): (u32, u32),
    style: CaptionStyle,
    window: &CaptionWindow,
) -> Result<(String, WrittenCaptions)> {
    let written = write_caption_files(tmp_dir, id, style.caption, window, style.font_path)?;
    let core = core_chain(width, fps, &written.spec(style));
    Ok((core, written))
}

/// Run a full export end to end, writing the finished file to `job.out_path`. Layers
/// that use an uploaded picture fail here (there is nowhere to look it up): use
/// [`run_export_with`] for those.
pub async fn run_export(
    tools: Tools<'_>,
    job: JobSpec<'_>,
    file: &Path,
    req: &ExportRequest,
    cues: &[Cue],
    font_path: &str,
) -> Result<()> {
    run_export_with(tools, job, file, req, cues, font_path, &|_| None).await
}

/// [`run_export`], with `media` to find the file behind an uploaded picture's id.
pub async fn run_export_with(
    tools: Tools<'_>,
    job: JobSpec<'_>,
    file: &Path,
    req: &ExportRequest,
    cues: &[Cue],
    font_path: &str,
    media: &(dyn Fn(&str) -> Option<LayerSource> + Sync),
) -> Result<()> {
    job.prepare_dirs()?;
    let style = CaptionStyle::new(req.caption.as_ref(), font_path, height_hint_for(req.width));
    let window = CaptionWindow {
        cues,
        start_ms: req.start_ms,
        end_ms: req.end_ms,
        offset_ms: req.subtitle_offset_ms,
    };
    let clip_ms = req.end_ms - req.start_ms;
    if let Some(why) = validate_grade(&req.grade, clip_ms)
        .or_else(|| validate_speed(req.speed))
        .or_else(|| validate_crop(req.crop.as_ref()))
    {
        anyhow::bail!(why);
    }
    // Everything is laid out on the whole picture, wide enough that the cropped window comes out the
    // width asked for; the crop and the speed are the last things done to it.
    let compose = compose_width(req.width, req.crop.as_ref());
    let finishing = finishing_filters(req.width, req.fps, req.crop.as_ref(), req.speed);
    let mixdown = Mixdown {
        speed: req.speed,
        ..req.mixdown
    };
    if let Some(why) = validate_music(
        req.music.as_ref(),
        req.clip_volume,
        clip_ms,
        req.format,
        req.audio,
    )
    .or_else(|| validate_clip_curve(&req.clip_curve, clip_ms, req.audio))
    .or_else(|| validate_mixdown(&mixdown, req.audio))
    {
        anyhow::bail!(why);
    }
    // The sound is mixed when there is a song or the clip's own volume was changed.
    let mix = if req.music.is_some()
        || !req.clip_curve.is_empty()
        || !mixdown.is_plain()
        || (req.clip_volume - 1.0).abs() > f32::EPSILON
    {
        let own = req.audio
            && ffmpeg::probe(tools.ffprobe, file)
                .await
                .map(|p| p.has_audio)
                .unwrap_or(false);
        plan_audio_mixed(
            req.music.as_ref(),
            req.clip_volume,
            &req.clip_curve,
            &mixdown,
            own,
            clip_ms,
            req.layers.len() + 1,
        )
    } else {
        None
    };
    let audio_mode = if req.audio || mix.is_some() {
        AudioMode::Encode
    } else {
        AudioMode::None
    };
    let codec = codec_args(req.format, req.quality, audio_mode);
    let title = job.source_title.filter(|_| req.credit_source);
    let out = job.out_path.to_string_lossy();
    // The sprite files (written for shape layers) go when this does.
    let mut sprites = SpriteFiles::default();
    let (args, caption_files) = if req.layers.is_empty() {
        let caption_files =
            write_caption_files(job.tmp_dir, job.id, style.caption, &window, style.font_path)?;
        let mut core = core_chain_graded(
            compose,
            req.fps,
            &caption_files.spec(style),
            &req.grade,
            clip_ms,
        );
        for f in &finishing {
            core.push(',');
            core.push_str(f);
        }
        let vf = video_filter(req.format, &core);
        let args = export_args(
            &file.to_string_lossy(),
            req.start_ms,
            req.end_ms,
            &vf,
            &codec,
            title,
            &out,
        );
        (args, caption_files)
    } else {
        if let Some(why) = validate_layers(&req.layers, clip_ms) {
            anyhow::bail!(why);
        }
        let caption_files =
            write_caption_files(job.tmp_dir, job.id, style.caption, &window, style.font_path)?;
        let (sources, written) = resolve_layers(&req.layers, job.tmp_dir, job.id, media)?;
        sprites = written;
        let mut after_chain = caption_filters(&caption_files.spec(style));
        after_chain.extend(req.grade.fade_filters(clip_ms));
        after_chain.extend(finishing.clone());
        let after = finishing_filter(req.format, &after_chain.join(","));
        let base = layers_base_graded(compose, req.fps, &req.grade);
        let plan = plan_layers_on(&req.layers, &sources, &base, compose, clip_ms, &after);
        let args = export_args_with_layers(
            &file.to_string_lossy(),
            req.start_ms,
            req.end_ms,
            &plan,
            req.audio,
            &codec,
            title,
            &out,
        );
        (args, caption_files)
    };
    let args = match &mix {
        Some(m) => splice_audio(args, m),
        None => args,
    };
    let result = ffmpeg::run(tools.ffmpeg, &args).await;
    caption_files.cleanup();
    drop(sprites);
    result?;
    maybe_optimize_gif(tools.gifsicle, req.format, req.quality, job.out_path).await;
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
    /// The font file for this clip's caption when it chose one; otherwise the
    /// sequence-wide default font is used.
    pub font_path: Option<String>,
    /// Pictures and shapes over this clip.
    pub layers: Vec<Layer>,
    /// A song under this clip only (its `file` filled in before the export runs).
    pub music: Option<Soundtrack>,
    /// The look of this clip's picture.
    pub grade: Grade,
    /// How this clip comes in over the one before it.
    pub transition: Option<Transition>,
    /// How fast this clip plays.
    pub speed: f32,
    /// The window of its picture to keep.
    pub crop: Option<Crop>,
}

/// What every clip of one sequence is rendered to: the frame rate, the shared
/// canvas size, and whether the sound is kept.
#[derive(Clone, Copy)]
pub struct SequenceLook {
    pub fps: u32,
    pub canvas: (u32, u32),
    pub audio: bool,
}

/// Render one sequence clip to an intermediate file matched to the shared
/// canvas size (letterboxed to preserve its own aspect ratio), with its own
/// caption burned in. Decent fixed quality — this is an intermediate; the
/// sequence's chosen quality is applied once, in the final encode pass.
///
/// When `audio` is set, the part carries uncompressed PCM audio; phase 3
/// does the one lossy Opus encode over the joined result.
pub(super) async fn render_sequence_clip(
    tools: Tools<'_>,
    tmp_dir: &Path,
    id: &str,
    clip: &ResolvedSequenceClip,
    look: SequenceLook,
    font_path: &str,
    media: &(dyn Fn(&str) -> Option<LayerSource> + Sync),
) -> Result<PathBuf> {
    let SequenceLook {
        fps,
        canvas: (width, height),
        audio,
    } = look;
    let window = CaptionWindow {
        cues: &clip.cues,
        start_ms: clip.start_ms,
        end_ms: clip.end_ms,
        offset_ms: clip.subtitle_offset_ms,
    };
    let caption_files =
        write_caption_files(tmp_dir, id, clip.caption.as_ref(), &window, font_path)?;
    let style = CaptionStyle::new(clip.caption.as_ref(), font_path, height);
    let clip_ms = clip.end_ms - clip.start_ms;
    let reframe = Reframe {
        crop: clip.crop.as_ref(),
        speed: clip.speed,
    };
    let core = clip_core_chain_graded(
        (width, height),
        fps,
        &caption_files.spec(style),
        &clip.grade,
        clip_ms,
        reframe,
    );
    let out_path = tmp_dir.join(format!("{id}.part.{}", part_ext(audio)));

    // The sprite files (written for shape layers) go when this does.
    let mut sprites = SpriteFiles::default();
    let args = if !clip.layers.is_empty() {
        let (sources, written) = match resolve_layers(&clip.layers, tmp_dir, id, media) {
            Ok(r) => r,
            Err(e) => {
                caption_files.cleanup();
                return Err(e);
            }
        };
        sprites = written;
        // the picture is fitted to the canvas, the layers go on that, and the captions on top
        let base = clip_base_graded((width, height), fps, &clip.grade, clip.crop.as_ref());
        let mut post = caption_filters(&caption_files.spec(style));
        post.extend(clip.grade.fade_filters(clip_ms));
        post.extend(speed_filters(fps, clip.speed));
        let post = post.join(",");
        let plan = plan_layers_on(&clip.layers, &sources, &base, width, clip_ms, &post);
        // as for a clip without layers, a source with no sound of its own is given silence
        let has_audio = audio
            && ffmpeg::probe(tools.ffprobe, &clip.file)
                .await
                .map(|p| p.has_audio)
                .unwrap_or(false);
        sequence_clip_args_with_layers(
            &clip.file,
            clip.start_ms,
            clip.end_ms,
            &plan,
            audio.then_some(has_audio),
            &out_path,
        )
    } else if audio {
        // A probe failure just means "treat this clip as silent" rather
        // than failing the whole sequence over it -- the clip still
        // exports fine, just without sound for this one segment.
        let has_audio = ffmpeg::probe(tools.ffprobe, &clip.file)
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

    // the sound plays at the clip's speed too (the picture's is in its filters)
    let args = if audio && !is_normal_speed(clip.speed) {
        speed_audio_args(args, clip.speed)
    } else {
        args
    };
    let result = ffmpeg::run(tools.ffmpeg, &args).await;
    caption_files.cleanup();
    drop(sprites);
    result?;
    Ok(out_path)
}

/// `args` (ending in the output file) with the sound played at `speed` times: `-af` just before the output.
pub(super) fn speed_audio_args(mut args: Vec<String>, speed: f32) -> Vec<String> {
    let tempo = tempo_filter(speed);
    let tempo = tempo.trim_start_matches(',');
    if !tempo.is_empty() {
        let at = args.len().saturating_sub(1);
        args.splice(at..at, ["-af".to_string(), tempo.to_string()]);
    }
    args
}

/// The argv for one sequence clip whose picture is a layer graph. `audio` is `None` for a
/// silent intermediate, else whether the source has sound (when it has none, silence is
/// substituted so every part has the same streams, as in [`sequence_clip_args_with_audio`]).
pub(super) fn sequence_clip_args_with_layers(
    file: &Path,
    start_ms: i64,
    end_ms: i64,
    plan: &LayerPlan,
    audio: Option<bool>,
    out: &Path,
) -> Vec<String> {
    let dur = (end_ms - start_ms).max(0);
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-ss".into(),
        ms_to_secs(start_ms),
        "-t".into(),
        ms_to_secs(dur),
        "-i".into(),
        file.to_string_lossy().into_owned(),
    ];
    args.extend(plan.input_args.iter().cloned());
    let layer_inputs = plan.input_args.iter().filter(|a| *a == "-i").count();
    if audio == Some(false) {
        // the silent input comes after the layers
        args.extend([
            "-f".into(),
            "lavfi".into(),
            "-t".into(),
            ms_to_secs(dur),
            "-i".into(),
            "anullsrc=channel_layout=stereo:sample_rate=48000".into(),
        ]);
    }
    args.extend(["-filter_complex".into(), plan.graph.clone()]);
    args.extend(["-map".into(), "[out]".into()]);
    match audio {
        Some(true) => args.extend(["-map".into(), "0:a:0".into()]),
        Some(false) => args.extend([
            "-map".into(),
            format!("{}:a:0", layer_inputs + 1),
            "-shortest".into(),
        ]),
        None => {}
    }
    args.extend(codec_args(
        Format::Mp4,
        90,
        if audio.is_some() {
            AudioMode::Pcm
        } else {
            AudioMode::None
        },
    ));
    args.extend(privacy_args(None));
    args.push(out.to_string_lossy().into_owned());
    args
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
        // A second, silent input (input 1), bounded to the clip's own length
        // with `-t`: an unbounded `anullsrc` leaves ending the encode entirely
        // to `-shortest`, which hangs on some ffmpeg builds. `-shortest` stays
        // as a guard for a clip that runs past the end of its source file.
        args.extend([
            "-f".into(),
            "lavfi".into(),
            "-t".into(),
            ms_to_secs(dur),
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
pub async fn run_sequence_export(
    tools: Tools<'_>,
    job: JobSpec<'_>,
    req: &SequenceExportRequest,
    clips: &[ResolvedSequenceClip],
    font_path: &str,
) -> Result<()> {
    run_sequence_export_with(tools, job, req, clips, font_path, &|_| None).await
}

/// [`run_sequence_export`], with `media` to find the file behind an uploaded picture's id
/// (for clips with layers that use one).
pub async fn run_sequence_export_with(
    tools: Tools<'_>,
    job: JobSpec<'_>,
    req: &SequenceExportRequest,
    clips: &[ResolvedSequenceClip],
    font_path: &str,
    media: &(dyn Fn(&str) -> Option<LayerSource> + Sync),
) -> Result<()> {
    let SequenceExportRequest {
        format,
        width,
        fps,
        quality,
        audio,
        ..
    } = *req;
    let (tmp_dir, job_id, out_path) = (job.tmp_dir, job.id, job.out_path);
    job.prepare_dirs()?;
    // The songs on the clips are checked before any clip is rendered.
    let lens: Vec<(Option<&Soundtrack>, i64)> = clips
        .iter()
        .map(|c| (c.music.as_ref(), c.end_ms - c.start_ms))
        .collect();
    if let Some(why) = validate_clip_music(&lens, format, audio) {
        anyhow::bail!(why);
    }
    let canvas = canvas_dims(width);
    let look = SequenceLook { fps, canvas, audio };

    // Phase 1: render each clip to a canvas-matched intermediate.
    let mut parts = Vec::with_capacity(clips.len());
    for (i, clip) in clips.iter().enumerate() {
        let id = format!("{job_id}-{i}");
        let font = clip.font_path.as_deref().unwrap_or(font_path);
        match render_sequence_clip(tools, tmp_dir, &id, clip, look, font, media).await {
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
    // When clips come in with transitions they are blended instead (see `join_args`).
    let transitions: Vec<Option<Transition>> = clips.iter().map(|c| c.transition).collect();
    let blended = has_transitions(&transitions);
    let list_path = tmp_dir.join(format!("{job_id}-list.txt"));
    if !blended {
        if let Err(e) = std::fs::write(&list_path, concat_list(&parts)) {
            cleanup_paths(&parts);
            return Err(e.into());
        }
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
    if blended {
        // the parts' real lengths (a part is a frame or so off its nominal length), so each
        // transition starts where the clip before it really ends
        let mut lens = Vec::with_capacity(parts.len());
        for (part, clip) in parts.iter().zip(clips) {
            let nominal = played_ms(clip.end_ms - clip.start_ms, clip.speed);
            let real = ffmpeg::probe(tools.ffprobe, part)
                .await
                .map(|p| p.duration_ms)
                .unwrap_or(0);
            lens.push(if (real - nominal).abs() < 500 && real > 0 {
                real
            } else {
                nominal
            });
        }
        concat_args = join_args(&parts, &lens, &transitions, fps, audio, &concat_path);
    }
    if let Err(e) = ffmpeg::run(tools.ffmpeg, &concat_args).await {
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
    let probed = ffmpeg::probe(tools.ffprobe, &concat_path).await;
    let total_ms: i64 = match &probed {
        Ok(p) if p.duration_ms > 0 => p.duration_ms,
        _ => clips.iter().map(|c| c.end_ms - c.start_ms).sum(),
    };
    let core = core_chain(canvas.0, fps, &CaptionSpec::none(canvas.1));
    let vf = video_filter(format, &core);
    // A song under the whole sequence, and/or the clips' own volume: the same sound graph
    // as for a single clip, over the joined sound.
    // The sequence's own song, then each clip's, placed on the whole sequence's time.
    let mut songs: Vec<Placed> = Vec::new();
    if let Some(m) = req.music.as_ref() {
        songs.push(Placed {
            music: m,
            at_ms: m.at_ms,
            len_ms: total_ms - m.at_ms,
        });
    }
    // each clip's own song starts where its clip starts in the joined sequence (a transition
    // pulls a clip back over the one before it)
    let lens: Vec<i64> = clips
        .iter()
        .map(|c| played_ms(c.end_ms - c.start_ms, c.speed))
        .collect();
    let starts = clip_starts(&lens, &transitions);
    for (c, offset) in clips.iter().zip(starts) {
        let len = c.end_ms - c.start_ms;
        if let Some(m) = c.music.as_ref() {
            songs.push(Placed {
                music: m,
                at_ms: offset + m.at_ms,
                len_ms: len - m.at_ms,
            });
        }
    }
    let mix = if !songs.is_empty() || (req.clip_volume - 1.0).abs() > f32::EPSILON {
        if let Some(why) =
            validate_music(req.music.as_ref(), req.clip_volume, total_ms, format, audio)
        {
            let _ = std::fs::remove_file(&concat_path);
            anyhow::bail!(why);
        }
        plan_mix(&songs, req.clip_volume, audio, total_ms, 1)
    } else {
        None
    };
    let codec = codec_args(
        format,
        quality,
        if audio || mix.is_some() {
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
        job.source_title,
        &out_path.to_string_lossy(),
    );
    let args = match &mix {
        Some(m) => splice_audio(args, m),
        None => args,
    };
    let result = ffmpeg::run(tools.ffmpeg, &args).await;
    let _ = std::fs::remove_file(&concat_path);
    result?;
    maybe_optimize_gif(tools.gifsicle, format, quality, out_path).await;
    Ok(())
}

pub(super) fn cleanup_paths(paths: &[PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}
