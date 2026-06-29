use std::fs;
use std::io::{self, Seek, SeekFrom};

pub(crate) const MAX_LOG_FILE_BYTES: u64 = 30 * 1024 * 1024;

pub(crate) fn clear_if_append_would_exceed(
    file: &mut fs::File,
    incoming_bytes: u64,
) -> io::Result<()> {
    let current_len = file.metadata()?.len();
    if current_len.saturating_add(incoming_bytes) > MAX_LOG_FILE_BYTES {
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
    } else {
        file.seek(SeekFrom::End(0))?;
    }
    Ok(())
}
