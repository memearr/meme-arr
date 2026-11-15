/** The buttons that finish the clip: save it back to its sequence, or name it, queue it and export. */
export function ExportBar({
  sequenceEdit,
  tooLong,
  handleSaveToSequence,
  onClose,
  filename,
  setFilename,
  handleAddToSequence,
  doExport,
  format,
}) {
  return (
    <>
      {sequenceEdit ? (
        <div className="export-row">
          <button type="button" className="export" onClick={() => handleSaveToSequence(false)} disabled={tooLong}>
            Save to sequence
          </button>
          {sequenceEdit.hasNext && (
            <button type="button" className="secondary" onClick={() => handleSaveToSequence(true)} disabled={tooLong}>
              Save and edit next clip
            </button>
          )}
          <button type="button" className="secondary" onClick={onClose}>
            Back without saving
          </button>
        </div>
      ) : null}
      {!sequenceEdit && (
        <div className="export-row">
          <input
            className="filename"
            placeholder="meme name (optional)"
            value={filename}
            onChange={(e) => setFilename(e.target.value)}
          />
          <button type="button" className="secondary" onClick={handleAddToSequence} disabled={tooLong}>
            🔗 Add to sequence
          </button>
          <button className="export" onClick={doExport} disabled={tooLong}>
            Export {format.toUpperCase()}
          </button>
        </div>
      )}
    </>
  );
}
