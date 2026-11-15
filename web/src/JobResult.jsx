export function JobResult({ job }) {
  if (job.status === "failed") return <p className="error">Export failed: {job.error}</p>;
  if (job.status !== "done") return <p className="hint">Rendering… ({job.status})</p>;
  const url = job.download_path;
  const isVideo = job.format === "mp4" || job.format === "webm";
  return (
    <div className="result-out">
      <p>
        Done: <b>{job.filename}</b>
      </p>
      <div className="out-preview">
        {isVideo ? (
          <video src={url} controls autoPlay loop muted={!job.audio} />
        ) : (
          <img src={url} alt={job.filename} />
        )}
      </div>
      <a className="download" href={url} download={job.filename}>
        ⬇ Download {job.filename}
      </a>
    </div>
  );
}
