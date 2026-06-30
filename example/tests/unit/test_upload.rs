/// Unit tests for upload module

#[cfg(test)]
mod tests {
    use super::super::upload::*;

    #[test]
    fn validate_valid_upload() {
        let req = UploadRequest {
            filename: "report.pdf".into(),
            size_bytes: 1024,
            data: vec![0u8; 1024],
        };
        assert!(validate_upload(&req).is_ok());
    }

    #[test]
    fn validate_empty_filename_fails() {
        let req = UploadRequest {
            filename: "".into(),
            size_bytes: 1024,
            data: vec![],
        };
        assert!(validate_upload(&req).is_err());
    }

    #[test]
    fn validate_too_large_fails() {
        let req = UploadRequest {
            filename: "big.bin".into(),
            size_bytes: 200 * 1024 * 1024,
            data: vec![],
        };
        assert!(validate_upload(&req).is_err());
    }
}
