//! Preview clips, single and sequence exports, jobs, and downloads.

use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct PreviewParams {
    start: i64,
    end: i64,
    #[serde(default = "preview_width")]
    width: u32,
    #[serde(default = "preview_fps")]
    fps: u32,
    /// "none" | "source" | "meme" — mirrors `export::Caption`, flattened for a
    /// GET query string so the preview can burn in the same caption the
    /// editor currently has selected.
    #[serde(default = "preview_caption_mode")]
    caption_mode: String,
    #[serde(default)]
    top: Option<String>,
    #[serde(default)]
    bottom: Option<String>,
    #[serde(default = "preview_font_size")]
    font_size: u32,
    /// A font by file name (see `GET /api/fonts`); absent = the default font.
    #[serde(default)]
    font: Option<String>,
    /// Text size, percent of normal.
    #[serde(default = "preview_font_scale")]
    font_scale: u32,
    /// Subtitle sync correction, same convention as `ExportRequest::subtitle_offset_ms`.
    #[serde(default)]
    offset_ms: i64,
    /// JSON-encoded `Vec<subtitles::Cue>` — the editor's edited line list
    /// (corrected/shortened/reworded), burned instead of the indexed lines
    /// when present. Query-string-friendly stand-in for a GET request body.
    #[serde(default)]
    cues_json: Option<String>,
}

pub(super) fn preview_width() -> u32 {
    640
}

pub(super) fn preview_fps() -> u32 {
    15
}

pub(super) fn preview_caption_mode() -> String {
    "none".into()
}

pub(super) fn preview_font_size() -> u32 {
    36
}

pub(super) fn preview_font_scale() -> u32 {
    100
}

pub(super) async fn preview(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
    Query(p): Query<PreviewParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if p.end <= p.start {
        return Ok(bad_request("end must be greater than start"));
    }
    // Cap preview length so a mis-set window can't transcode an entire film.
    let end = p.end.min(p.start + 60_000);

    let pool = st.pool.clone();
    let rk = rating_key.clone();
    let m = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::get_media(&conn, &rk)
    })
    .await??;
    let m = match m {
        Some(m) => m,
        None => return Ok(not_found("media not indexed")),
    };

    let direct = match paths::resolve(&m.file_path, &st.mappings()) {
        paths::AccessMode::Direct(p) => p,
        paths::AccessMode::NotMounted => {
            return Ok(bad_request(
                "source file is not on a mounted volume; check your path mappings in Settings",
            ))
        }
    };

    let override_cues: Option<Vec<subtitles::Cue>> = match &p.cues_json {
        Some(s) if !s.is_empty() => {
            Some(serde_json::from_str(s).map_err(|e| anyhow::anyhow!("invalid cues_json: {e}"))?)
        }
        _ => None,
    };

    let caption: Option<export::Caption> = match p.caption_mode.as_str() {
        "source" => Some(export::Caption::Source {
            cues: override_cues,
            font: p.font.clone(),
            font_scale: p.font_scale,
        }),
        "meme" => Some(export::Caption::Meme {
            top: p.top.clone(),
            bottom: p.bottom.clone(),
            font_size: p.font_size,
            font: p.font.clone(),
            font_scale: p.font_scale,
        }),
        _ => None,
    };

    // Only fetch cues for a "source" preview with no edited override, widened
    // by the offset so a cue shifted into/out of view is among the candidates
    // trimmed_srt sees. An override needs no DB round trip at all.
    let cues = match &caption {
        Some(export::Caption::Source { cues: None, .. }) => {
            let pool = st.pool.clone();
            let media_id = m.id;
            let pad = p.offset_ms.abs();
            let (s, e) = (p.start, end);
            tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                let conn = pool.get()?;
                db::cues_in_window(&conn, media_id, s - pad, e + pad)
            })
            .await??
        }
        _ => Vec::new(),
    };

    let font_path = match caption_font_path(&st, caption.as_ref()) {
        Ok(p) => p,
        Err(msg) => return Ok(bad_request(msg)),
    };

    // Cache previews by everything that affects the rendered frames: file,
    // window, size/fps, and the caption (mode/text/offset).
    let tmp = st.config.tmp_dir();
    std::fs::create_dir_all(&tmp)?;
    let mut h = DefaultHasher::new();
    (
        (&m.file_path, p.start, end, p.width, p.fps, &p.caption_mode),
        (&p.top, &p.bottom, p.font_size, &p.font, p.font_scale),
        (p.offset_ms, &p.cues_json),
    )
        .hash(&mut h);
    let hash = h.finish();
    let cache = tmp.join(format!("preview-{hash:x}.mp4"));

    // One request renders a given clip while identical ones wait; the render
    // goes to a temporary file that only becomes the cache entry once it is a
    // complete video, so a failed, cancelled or concurrent request can never
    // leave (or serve) a half-written preview. See `preview_cache`.
    let t0 = std::time::Instant::now();
    let title = m.title.clone();
    preview_cache::ensure_rendered(hash, &cache, |out| async move {
        let frame_height_hint = (p.width as f32 * 0.5625) as u32;
        let (vf, caption_files) = export::prepare_captions(
            &tmp,
            &format!("preview-{hash:x}"),
            p.width,
            p.fps,
            caption.as_ref(),
            &cues,
            p.start,
            end,
            p.offset_ms,
            &font_path,
            frame_height_hint,
        )?;
        let args = ffmpeg::preview_args(
            &direct.to_string_lossy(),
            p.start,
            end,
            &vf,
            &out.to_string_lossy(),
        );
        let result = ffmpeg::run(&st.config.ffmpeg_bin, &args).await;
        caption_files.cleanup();
        if let Err(e) = &result {
            warn!("preview render for {title} failed: {e:#}");
        }
        result
    })
    .await
    .map_err(|e| anyhow::anyhow!("couldn't render the preview: {e:#}"))?;
    tracing::debug!(
        "preview for {} ready in {} ms",
        m.title,
        t0.elapsed().as_millis()
    );
    serve_file(&cache, "video/mp4", &headers).await
}

pub(super) async fn export_start(
    State(st): State<AppState>,
    Json(req): Json<ExportRequest>,
) -> Result<Response, AppError> {
    if req.end_ms <= req.start_ms {
        return Ok(bad_request("end must be greater than start"));
    }
    if req.end_ms - req.start_ms > 60_000 {
        return Ok(bad_request("clips are limited to 60 seconds"));
    }
    if let Some(msg) = export::validate_audio_format(req.format, req.audio) {
        return Ok(bad_request(msg));
    }

    // Resolve the source file now so we can fail fast with a clear error.
    let pool = st.pool.clone();
    let rk = req.rating_key.clone();
    let m = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::get_media(&conn, &rk)
    })
    .await??;
    let m = match m {
        Some(m) => m,
        None => return Ok(not_found("media not indexed")),
    };
    let direct = match paths::resolve(&m.file_path, &st.mappings()) {
        paths::AccessMode::Direct(p) => p,
        paths::AccessMode::NotMounted => {
            return Ok(bad_request(
                "source file is not on a mounted volume; check your path mappings in Settings",
            ))
        }
    };

    let job_id = uuid::Uuid::new_v4().to_string();
    let filename = export::safe_filename(req.filename.as_deref(), req.format, &job_id[..8]);
    let out_path = st.config.output_dir.join(&filename);

    st.set_job(Job {
        id: job_id.clone(),
        status: JobStatus::Queued,
        format: req.format,
        filename: filename.clone(),
        audio: req.audio,
        download_path: None,
        error: None,
    });

    // Gather cues for burned subtitles — only for "source" captions with no
    // edited override (an override needs no DB round trip; `run_export` uses
    // it directly from `req.caption`). Widen by the sync offset so a cue
    // shifted into/out of view is among the candidates `trimmed_srt` sees.
    let cues = if matches!(
        req.caption,
        Some(export::Caption::Source { cues: None, .. })
    ) {
        let pool = st.pool.clone();
        let media_id = m.id;
        let (start, end) = (req.start_ms, req.end_ms);
        let pad = req.subtitle_offset_ms.abs();
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let conn = pool.get()?;
            db::cues_in_window(&conn, media_id, start - pad, end + pad)
        })
        .await??
    } else {
        Vec::new()
    };

    let font_path = match caption_font_path(&st, req.caption.as_ref()) {
        Ok(p) => p,
        Err(msg) => return Ok(bad_request(msg)),
    };
    let frame_height_hint = (req.width as f32 * 0.5625) as u32;
    let tmp = st.config.tmp_dir();
    let ffmpeg_bin = st.config.ffmpeg_bin.clone();
    let gifsicle_bin = st.config.gifsicle_bin.clone();
    let st2 = st.clone();
    let jid = job_id.clone();
    let dl = format!("{}/api/download/{filename}", st.config.url_base);
    // The one tag the file may carry: what it came from (from the library's
    // own metadata, not the source file's tags).
    let source_title = export::source_label(
        &m.kind,
        &m.title,
        m.show.as_deref(),
        m.season,
        m.episode,
        m.year,
    );

    tokio::spawn(async move {
        st2.update_job(&jid, |j| j.status = JobStatus::Running);
        let res = export::run_export(
            &ffmpeg_bin,
            &gifsicle_bin,
            &tmp,
            &jid,
            &direct,
            &req,
            &cues,
            &font_path,
            frame_height_hint,
            Some(&source_title),
            &out_path,
        )
        .await;
        match res {
            Ok(()) => {
                info!("export complete: {}", out_path.display());
                st2.update_job(&jid, |j| {
                    j.status = JobStatus::Done;
                    j.download_path = Some(dl);
                });
            }
            Err(e) => {
                report_export_failure(&st2, "Export", &out_path, &e);
                st2.update_job(&jid, |j| {
                    j.status = JobStatus::Failed;
                    j.error = Some(format!("{e:#}"));
                });
            }
        }
    });

    Ok((StatusCode::ACCEPTED, Json(st.get_job(&job_id))).into_response())
}

/// Record a failed export somewhere that outlives the job: `tracing` for the
/// server log and the persistent Activity log for the UI. The job's own
/// `error` field is in-memory and gone after a restart.
pub(super) fn report_export_failure(
    st: &AppState,
    what: &str,
    out_path: &std::path::Path,
    e: &anyhow::Error,
) {
    warn!("{what} failed: {}: {e:#}", out_path.display());
    let name = out_path.file_name().unwrap_or_default().to_string_lossy();
    st.log_event(LogLevel::Error, format!("{what} failed ({name}): {e:#}"));
}

/// Resolving a sequence can fail per-clip in a way the caller can fix (media
/// not indexed, file not mounted) as opposed to an actual server error.
pub(super) type SequenceResolveResult =
    anyhow::Result<std::result::Result<Vec<export::ResolvedSequenceClip>, String>>;

pub(super) async fn sequence_export_start(
    State(st): State<AppState>,
    Json(req): Json<export::SequenceExportRequest>,
) -> Result<Response, AppError> {
    if let Err(msg) = export::validate_sequence(&req.clips) {
        return Ok(bad_request(msg));
    }
    if let Some(msg) = export::validate_audio_format(req.format, req.audio) {
        return Ok(bad_request(msg));
    }

    // Resolve every clip's media row, mounted path, and (for "source" caption
    // clips) its own subtitle cues, in one connection round trip.
    let pool = st.pool.clone();
    let mappings = st.mappings();
    let clip_specs = req.clips.clone();
    let fonts = st.config.fonts();
    let resolved = tokio::task::spawn_blocking(move || -> SequenceResolveResult {
        let conn = pool.get()?;
        let mut out = Vec::with_capacity(clip_specs.len());
        for (i, spec) in clip_specs.iter().enumerate() {
            let media = match db::get_media(&conn, &spec.rating_key)? {
                Some(m) => m,
                None => {
                    return Ok(Err(format!(
                        "clip #{} ({}): media not indexed",
                        i + 1,
                        spec.rating_key
                    )))
                }
            };
            let file = match paths::resolve(&media.file_path, &mappings) {
                paths::AccessMode::Direct(p) => p,
                paths::AccessMode::NotMounted => {
                    return Ok(Err(format!(
                        "clip #{} ({}): source file is not on a mounted volume; check your path mappings in Settings",
                        i + 1,
                        spec.rating_key
                    )))
                }
            };
            // An edited line list needs no DB round trip; write_caption_files
            // uses it straight from `caption` via ResolvedSequenceClip.
            let cues = if matches!(spec.caption, Some(export::Caption::Source { cues: None, .. })) {
                // Widen the query by the offset so a cue that shifts into view
                // (or out of it) is actually among the candidates trimmed_srt
                // sees — the offset itself is applied there, not here.
                let pad = spec.subtitle_offset_ms.abs();
                db::cues_in_window(&conn, media.id, spec.start_ms - pad, spec.end_ms + pad)?
            } else {
                Vec::new()
            };
            // A clip may caption in its own font; the others use the default.
            let font_path = match spec.caption.as_ref().and_then(|c| c.font_id()) {
                Some(id) => match fonts.path_of(id) {
                    Some(p) => Some(p.to_string_lossy().to_string()),
                    None => {
                        return Ok(Err(format!(
                            "clip #{}: unknown font {id:?}; see Settings -> Fonts",
                            i + 1
                        )))
                    }
                },
                None => None,
            };
            out.push(export::ResolvedSequenceClip {
                font_path,
                file,
                start_ms: spec.start_ms,
                end_ms: spec.end_ms,
                caption: spec.caption.clone(),
                subtitle_offset_ms: spec.subtitle_offset_ms,
                cues,
                source: export::source_label(
                    &media.kind,
                    &media.title,
                    media.show.as_deref(),
                    media.season,
                    media.episode,
                    media.year,
                ),
            });
        }
        Ok(Ok(out))
    })
    .await??;

    let clips = match resolved {
        Ok(c) => c,
        Err(msg) => return Ok(bad_request(msg)),
    };

    let job_id = uuid::Uuid::new_v4().to_string();
    let filename = export::safe_filename(req.filename.as_deref(), req.format, &job_id[..8]);
    let out_path = st.config.output_dir.join(&filename);

    st.set_job(Job {
        id: job_id.clone(),
        status: JobStatus::Queued,
        format: req.format,
        filename: filename.clone(),
        audio: req.audio,
        download_path: None,
        error: None,
    });

    let font_path = default_font(&st.config.fonts_dir);
    let tmp = st.config.tmp_dir();
    let ffmpeg_bin = st.config.ffmpeg_bin.clone();
    let gifsicle_bin = st.config.gifsicle_bin.clone();
    let ffprobe_bin = st.config.ffprobe_bin.clone();
    let (width, fps, quality, format, audio) =
        (req.width, req.fps, req.quality, req.format, req.audio);
    // The one tag the sequence may carry: each source it was cut from, once.
    let source_title = if req.credit_source {
        export::combined_source_title(&clips.iter().map(|c| c.source.clone()).collect::<Vec<_>>())
    } else {
        None
    };
    let st2 = st.clone();
    let jid = job_id.clone();
    let dl = format!("{}/api/download/{filename}", st.config.url_base);

    tokio::spawn(async move {
        st2.update_job(&jid, |j| j.status = JobStatus::Running);
        let res = export::run_sequence_export(
            &ffmpeg_bin,
            &gifsicle_bin,
            &ffprobe_bin,
            &tmp,
            &jid,
            &clips,
            format,
            width,
            fps,
            quality,
            audio,
            &font_path,
            source_title.as_deref(),
            &out_path,
        )
        .await;
        match res {
            Ok(()) => {
                info!("sequence export complete: {}", out_path.display());
                st2.update_job(&jid, |j| {
                    j.status = JobStatus::Done;
                    j.download_path = Some(dl);
                });
            }
            Err(e) => {
                report_export_failure(&st2, "Sequence export", &out_path, &e);
                st2.update_job(&jid, |j| {
                    j.status = JobStatus::Failed;
                    j.error = Some(format!("{e:#}"));
                });
            }
        }
    });

    Ok((StatusCode::ACCEPTED, Json(st.get_job(&job_id))).into_response())
}

pub(super) async fn job_status(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    match st.get_job(&id) {
        Some(j) => Ok(Json(j).into_response()),
        None => Ok(not_found("no such job")),
    }
}

pub(super) async fn download(
    State(st): State<AppState>,
    Path(filename): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    // Reject anything that isn't a plain file name.
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Ok(bad_request("invalid filename"));
    }
    let path = st.config.output_dir.join(&filename);
    if !path.is_file() {
        return Ok(not_found("file not found"));
    }
    let ct = content_type_for(&filename);
    let mut resp = serve_file(&path, ct, &headers).await?;
    resp.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or(header::HeaderValue::from_static("attachment")),
    );
    Ok(resp)
}
