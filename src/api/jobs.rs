//! Background export jobs: queue one, run its work, and report how it ended.
//! Single-clip and sequence exports differ only in the work they do, so the
//! bookkeeping around it lives here once.

use super::*;
use std::future::Future;
use std::path::PathBuf;

/// The finished file of a job: its name, where it is written, and the address
/// it is downloaded from.
pub(super) struct JobFiles {
    pub filename: String,
    pub out_path: PathBuf,
    pub download: String,
}

impl JobFiles {
    /// `requested` (made safe) or a name from the job id, with the format's extension.
    pub fn new(
        st: &AppState,
        requested: Option<&str>,
        format: export::Format,
        job_id: &str,
    ) -> Self {
        Self::with_ext(st, requested, format.ext(), job_id)
    }

    /// [`JobFiles::new`] for any file extension.
    pub fn with_ext(st: &AppState, requested: Option<&str>, ext: &str, job_id: &str) -> Self {
        let filename = export::safe_filename_ext(requested, ext, &job_id[..8]);
        JobFiles {
            out_path: st.config.output_dir.join(&filename),
            download: format!("{}/api/download/{filename}", st.config.url_base),
            filename,
        }
    }
}

/// The programs the exports run, as configured.
pub(super) fn tools(st: &AppState) -> export::Tools<'_> {
    export::Tools {
        ffmpeg: &st.config.ffmpeg_bin,
        ffprobe: &st.config.ffprobe_bin,
        gifsicle: &st.config.gifsicle_bin,
    }
}

/// Register a new queued job: its id, and the file it will produce.
pub(super) fn queue(
    st: &AppState,
    requested_name: Option<&str>,
    format: export::Format,
    audio: bool,
) -> (String, JobFiles) {
    let id = uuid::Uuid::new_v4().to_string();
    let files = JobFiles::new(st, requested_name, format, &id);
    st.set_job(Job {
        id: id.clone(),
        status: JobStatus::Queued,
        format,
        filename: files.filename.clone(),
        audio,
        download_path: None,
        error: None,
    });
    (id, files)
}

/// Run `work` in the background for job `id`: mark it running, then done (with
/// its download address) or failed (logged, and kept on the job).
/// `what` names the job in the logs, e.g. "Export" or "Sequence export".
pub(super) fn run_in_background<F>(
    st: &AppState,
    id: String,
    what: &'static str,
    files: JobFiles,
    work: F,
) where
    F: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let st = st.clone();
    tokio::spawn(async move {
        st.update_job(&id, |j| j.status = JobStatus::Running);
        match work.await {
            Ok(()) => {
                info!(
                    "{} complete: {}",
                    what.to_lowercase(),
                    files.out_path.display()
                );
                st.update_job(&id, |j| {
                    j.status = JobStatus::Done;
                    j.download_path = Some(files.download);
                });
            }
            Err(e) => {
                report_export_failure(&st, what, &files.out_path, &e);
                st.update_job(&id, |j| {
                    j.status = JobStatus::Failed;
                    j.error = Some(format!("{e:#}"));
                });
            }
        }
    });
}

/// `202 Accepted` with the job as it stands.
pub(super) fn accepted(st: &AppState, id: &str) -> Response {
    (StatusCode::ACCEPTED, Json(st.get_job(id))).into_response()
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
