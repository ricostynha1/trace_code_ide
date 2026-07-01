/// File upload module

use std::path::PathBuf;

pub struct UploadRequest {
    pub filename: String,
    pub size_bytes: u64,
    pub data: Vec<u8>,
}

const MAX_SIZE: u64 = 100 * 1024 * 1024; // 100MB

pub fn validate_upload(req: &UploadRequest) -> Result<(), UploadError> {
    if req.filename.is_empty() {
        return Err(UploadError::EmptyFilename);
    }
    if req.size_bytes > MAX_SIZE {
        return Err(UploadError::TooLarge);
    }
    Ok(())
}

pub fn save_upload(req: &UploadRequest, workspace: &PathBuf) -> Result<PathBuf, UploadError> {
    validate_upload(req)?;
    let dest = workspace.join(&req.filename);
    // In real impl: write data to dest
    Ok(dest)
}

pub enum UploadError {
    EmptyFilename,
    TooLarge,
    IoError(String),
}

AbcdFestas Amen