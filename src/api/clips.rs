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
    /// JSON-encoded `export::TextLook` for burned subtitles (colour, place,
    /// word-by-word reveal); absent is the plain subtitle look.
    #[serde(default)]
    look_json: Option<String>,
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

impl PreviewParams {
    /// The caption this preview asks for, from the flattened query fields.
    fn caption(&self) -> anyhow::Result<Option<export::Caption>> {
        let override_cues: Option<Vec<subtitles::Cue>> = match &self.cues_json {
            Some(s) if !s.is_empty() => Some(
                serde_json::from_str(s).map_err(|e| anyhow::anyhow!("invalid cues_json: {e}"))?,
            ),
            _ => None,
        };
        Ok(match self.caption_mode.as_str() {
            "source" => Some(export::Caption::Source {
                cues: override_cues,
                font: self.font.clone(),
                font_scale: self.font_scale,
                look: match &self.look_json {
                    Some(s) if !s.is_empty() => Some(
                        serde_json::from_str(s)
                            .map_err(|e| anyhow::anyhow!("invalid look_json: {e}"))?,
                    ),
                    _ => None,
                },
            }),
            "meme" => Some(export::Caption::Meme {
                top: self.top.clone(),
                bottom: self.bottom.clone(),
                font_size: self.font_size,
                font: self.font.clone(),
                font_scale: self.font_scale,
            }),
            _ => None,
        })
    }

    /// Everything that affects the rendered frames: file, window, size/fps and
    /// the caption (mode, text, font, offset). Same inputs, same cache entry.
    fn cache_key(&self, file_path: &str, end: i64) -> u64 {
        let mut h = DefaultHasher::new();
        (
            (
                file_path,
                self.start,
                end,
                self.width,
                self.fps,
                &self.caption_mode,
            ),
            (
                &self.top,
                &self.bottom,
                self.font_size,
                &self.font,
                self.font_scale,
            ),
            (self.offset_ms, &self.cues_json, &self.look_json),
        )
            .hash(&mut h);
        h.finish()
    }
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

    let (m, direct) = match resolve_source(&st, &rating_key).await? {
        Ok(found) => found,
        Err(resp) => return Ok(resp),
    };
    let caption = p.caption()?;
    let cues = source_cues_for(&st, m.id, caption.as_ref(), (p.start, end), p.offset_ms).await?;
    let font_path = match caption_font_path(&st, caption.as_ref()) {
        Ok(p) => p,
        Err(msg) => return Ok(bad_request(msg)),
    };

    let Some(tmp) = paths::confine(&st.config.data_dir, &st.config.tmp_dir()) else {
        return Err(anyhow::anyhow!("the data folder isn't usable").into());
    };
    std::fs::create_dir_all(&tmp)?;
    let hash = p.cache_key(&m.file_path, end);
    let cache = tmp.join(format!("preview-{hash:x}.mp4"));

    // One request renders a given clip while identical ones wait; the render
    // goes to a temporary file that only becomes the cache entry once it is a
    // complete video, so a failed, cancelled or concurrent request can never
    // leave (or serve) a half-written preview. See `preview_cache`.
    let t0 = std::time::Instant::now();
    let title = m.title.clone();
    preview_cache::ensure_rendered(hash, &cache, |out| async move {
        let style = export::CaptionStyle::new(
            caption.as_ref(),
            &font_path,
            export::height_hint_for(p.width),
        );
        let window = export::CaptionWindow {
            cues: &cues,
            start_ms: p.start,
            end_ms: end,
            offset_ms: p.offset_ms,
        };
        let (vf, caption_files) = export::prepare_captions(
            &tmp,
            &format!("preview-{hash:x}"),
            (p.width, p.fps),
            style,
            &window,
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
    Json(mut req): Json<ExportRequest>,
) -> Result<Response, AppError> {
    if req.end_ms <= req.start_ms {
        return Ok(bad_request("end must be greater than start"));
    }
    if req.end_ms - req.start_ms > export::MAX_CLIP_MS {
        return Ok(bad_request("clips are limited to 60 seconds"));
    }
    if let Some(msg) = export::validate_audio_format(req.format, req.audio) {
        return Ok(bad_request(msg));
    }
    if let Some(msg) = export::validate_layers(&req.layers, req.end_ms - req.start_ms) {
        return Ok(bad_request(msg));
    }
    if let Some(msg) = export::validate_music(
        req.music.as_ref(),
        req.clip_volume,
        req.end_ms - req.start_ms,
        req.format,
        req.audio,
    ) {
        return Ok(bad_request(msg));
    }
    if req.music.is_some() && st.settings().navidrome().is_none() {
        return Ok(bad_request("music needs Navidrome (Settings -> Navidrome)"));
    }
    if let Some(msg) = req
        .caption
        .as_ref()
        .and_then(|c| c.problem(req.end_ms - req.start_ms))
    {
        return Ok(bad_request(msg));
    }
    // An uploaded picture a layer names must still be there.
    let overlays = st.config.overlays();
    for (i, l) in req.layers.iter().enumerate() {
        if let export::LayerKind::Media { asset } = &l.kind {
            if overlays.path_of(asset).is_none() {
                return Ok(bad_request(format!(
                    "layer {}: the picture {asset:?} is not uploaded (see Settings -> Overlays)",
                    i + 1
                )));
            }
        }
    }

    // Everything that can be refused is checked before a job exists.
    let (m, file) = match resolve_source(&st, &req.rating_key).await? {
        Ok(found) => found,
        Err(resp) => return Ok(resp),
    };
    let window = (req.start_ms, req.end_ms);
    let cues = source_cues_for(
        &st,
        m.id,
        req.caption.as_ref(),
        window,
        req.subtitle_offset_ms,
    )
    .await?;
    let font_path = match caption_font_path(&st, req.caption.as_ref()) {
        Ok(p) => p,
        Err(msg) => return Ok(bad_request(msg)),
    };
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

    let (id, files) = jobs::queue(&st, req.filename.as_deref(), req.format, req.audio);
    let (st2, job_id, tmp, out_path) = (
        st.clone(),
        id.clone(),
        st.config.tmp_dir(),
        files.out_path.clone(),
    );
    run_in_background(&st, id.clone(), "Export", files, async move {
        let job = export::JobSpec {
            id: &job_id,
            tmp_dir: &tmp,
            out_path: &out_path,
            source_title: Some(&source_title),
        };
        let overlays = st2.config.overlays();
        let media = move |id: &str| {
            overlays.path_of(id).map(|path| export::LayerSource {
                path,
                motion: !crate::overlays::Overlays::is_still(id),
            })
        };
        // The song is fetched here, not before the job exists, so a big first fetch
        // doesn't hold up the request.
        if let Some(m) = req.music.as_mut() {
            m.file = Some(match ensure_track(&st2, &m.id).await {
                Ok(p) => p,
                Err(TrackError::NotSetUp) => anyhow::bail!("music needs Navidrome"),
                Err(TrackError::BadId) => anyhow::bail!("that isn't a song id"),
                Err(TrackError::Upstream(e)) => anyhow::bail!("couldn't get the song: {e}"),
            });
        }
        export::run_export_with(tools(&st2), job, &file, &req, &cues, &font_path, &media).await
    });
    Ok(accepted(&st, &id))
}

/// Resolving a sequence can fail per-clip in a way the caller can fix (media
/// not indexed, file not mounted) as opposed to an actual server error.
pub(super) type SequenceResolveResult =
    anyhow::Result<std::result::Result<Vec<export::ResolvedSequenceClip>, String>>;

/// Look up every clip's media row, mounted path, own font and (for "source"
/// caption clips) its subtitle cues, on one connection. A problem the caller
/// can fix comes back as the `Err` message, naming the clip.
fn resolve_sequence(
    conn: &rusqlite::Connection,
    specs: &[export::SequenceClipSpec],
    mappings: &[crate::config::PathRewrite],
    fonts: &crate::fonts::Fonts,
) -> SequenceResolveResult {
    let mut out = Vec::with_capacity(specs.len()); // bounded by validate_sequence
    for (i, spec) in specs.iter().enumerate() {
        let n = i + 1;
        let Some(media) = db::get_media(conn, &spec.rating_key)? else {
            return Ok(Err(format!(
                "clip #{n} ({}): media not indexed",
                spec.rating_key
            )));
        };
        let paths::AccessMode::Direct(file) = paths::resolve(&media.file_path, mappings) else {
            return Ok(Err(format!(
                "clip #{n} ({}): {NOT_MOUNTED}",
                spec.rating_key
            )));
        };
        let cues = source_cues(
            conn,
            media.id,
            spec.caption.as_ref(),
            (spec.start_ms, spec.end_ms),
            spec.subtitle_offset_ms,
        )?;
        // A clip may caption in its own font; the others use the default.
        let font_path = match spec.caption.as_ref().and_then(|c| c.font_id()) {
            Some(id) => match fonts.path_of(id) {
                Some(p) => Some(p.to_string_lossy().to_string()),
                None => {
                    return Ok(Err(format!(
                        "clip #{n}: unknown font {id:?}; see Settings -> Fonts"
                    )))
                }
            },
            None => None,
        };
        out.push(export::ResolvedSequenceClip {
            font_path,
            layers: spec.layers.clone(),
            music: spec.music.clone(),
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
}

pub(super) async fn sequence_export_start(
    State(st): State<AppState>,
    Json(mut req): Json<export::SequenceExportRequest>,
) -> Result<Response, AppError> {
    if let Err(msg) = export::validate_sequence(&req.clips) {
        return Ok(bad_request(msg));
    }
    if let Some(msg) = export::validate_audio_format(req.format, req.audio) {
        return Ok(bad_request(msg));
    }
    let total_ms: i64 = req.clips.iter().map(|c| c.end_ms - c.start_ms).sum();
    if let Some(msg) = export::validate_music(
        req.music.as_ref(),
        req.clip_volume,
        total_ms,
        req.format,
        req.audio,
    ) {
        return Ok(bad_request(msg));
    }
    let clip_songs: Vec<(Option<&export::Soundtrack>, i64)> = req
        .clips
        .iter()
        .map(|c| (c.music.as_ref(), c.end_ms - c.start_ms))
        .collect();
    if let Some(msg) = export::validate_clip_music(&clip_songs, req.format, req.audio) {
        return Ok(bad_request(msg));
    }
    if (req.music.is_some() || clip_songs.iter().any(|(m, _)| m.is_some()))
        && st.settings().navidrome().is_none()
    {
        return Ok(bad_request("music needs Navidrome (Settings -> Navidrome)"));
    }
    // An uploaded picture a layer names must still be there.
    let overlays = st.config.overlays();
    for (n, clip) in req.clips.iter().enumerate() {
        for (i, l) in clip.layers.iter().enumerate() {
            if let export::LayerKind::Media { asset } = &l.kind {
                if overlays.path_of(asset).is_none() {
                    return Ok(bad_request(format!(
                        "clip #{} layer {}: the picture {asset:?} is not uploaded (see Settings -> Overlays)",
                        n + 1,
                        i + 1
                    )));
                }
            }
        }
    }

    let (pool, mappings, specs, fonts) = (
        st.pool.clone(),
        st.mappings(),
        req.clips.clone(),
        st.config.fonts(),
    );
    let resolved = tokio::task::spawn_blocking(move || -> SequenceResolveResult {
        let conn = pool.get()?;
        resolve_sequence(&conn, &specs, &mappings, &fonts)
    })
    .await??;
    let clips = match resolved {
        Ok(c) => c,
        Err(msg) => return Ok(bad_request(msg)),
    };

    // The one tag the sequence may carry: each source it was cut from, once.
    let source_title = if req.credit_source {
        export::combined_source_title(&clips.iter().map(|c| c.source.clone()).collect::<Vec<_>>())
    } else {
        None
    };
    let (id, files) = jobs::queue(&st, req.filename.as_deref(), req.format, req.audio);
    let (st2, job_id, tmp, out_path, font_path) = (
        st.clone(),
        id.clone(),
        st.config.tmp_dir(),
        files.out_path.clone(),
        default_font(&st.config.fonts_dir),
    );
    run_in_background(&st, id.clone(), "Sequence export", files, async move {
        let job = export::JobSpec {
            id: &job_id,
            tmp_dir: &tmp,
            out_path: &out_path,
            source_title: source_title.as_deref(),
        };
        if let Some(m) = req.music.as_mut() {
            m.file = Some(match ensure_track(&st2, &m.id).await {
                Ok(p) => p,
                Err(TrackError::NotSetUp) => anyhow::bail!("music needs Navidrome"),
                Err(TrackError::BadId) => anyhow::bail!("that isn't a song id"),
                Err(TrackError::Upstream(e)) => anyhow::bail!("couldn't get the song: {e}"),
            });
        }
        let overlays = st2.config.overlays();
        let media = move |id: &str| {
            overlays.path_of(id).map(|path| export::LayerSource {
                path,
                motion: !crate::overlays::Overlays::is_still(id),
            })
        };
        // each clip's song too (a song used twice is fetched once)
        let mut clips = clips;
        for (n, c) in clips.iter_mut().enumerate() {
            if let Some(m) = c.music.as_mut() {
                m.file = Some(match ensure_track(&st2, &m.id).await {
                    Ok(p) => p,
                    Err(TrackError::NotSetUp) => anyhow::bail!("music needs Navidrome"),
                    Err(TrackError::BadId) => {
                        anyhow::bail!("clip #{}: that isn't a song id", n + 1)
                    }
                    Err(TrackError::Upstream(e)) => {
                        anyhow::bail!("clip #{}: couldn't get the song: {e}", n + 1)
                    }
                });
            }
        }
        export::run_sequence_export_with(tools(&st2), job, &req, &clips, &font_path, &media).await
    });
    Ok(accepted(&st, &id))
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
    // The file has to really live in the output folder (a symlink can't lead out of it).
    let Some(path) = paths::contained_file(&st.config.output_dir, &filename) else {
        return Ok(not_found("file not found"));
    };
    let ct = content_type_for(&filename);
    let mut resp = serve_file(&path, ct, &headers).await?;
    resp.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or(header::HeaderValue::from_static("attachment")),
    );
    Ok(resp)
}
