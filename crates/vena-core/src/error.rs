use thiserror::Error;

/// Every Tauri command returns `Result<T, VenaError>`. Serialized to the UI as
/// `{ code, message }` so the frontend can branch on `code` (e.g. show the
/// spoiler-consent gate when `SpoilerConsentRequired`).
#[derive(Debug, Error)]
pub enum VenaError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("spoiler consent required for this book before full-spoiler mode")]
    SpoilerConsentRequired,
    #[error("no AI backend is ready (download a local model or configure Cloud Relay)")]
    NoBackend,
    #[error("invalid package: {0}")]
    InvalidPackage(String),
    #[error("network destination not permitted by policy: {0}")]
    NetworkNotAllowed(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("package archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("inference error: {0}")]
    Inference(String),
    #[error("{0}")]
    Other(String),
}

impl VenaError {
    /// Stable machine code for the UI to switch on.
    pub fn code(&self) -> &'static str {
        match self {
            VenaError::NotFound(_) => "NotFound",
            VenaError::SpoilerConsentRequired => "SpoilerConsentRequired",
            VenaError::NoBackend => "NoBackend",
            VenaError::InvalidPackage(_) => "InvalidPackage",
            VenaError::NetworkNotAllowed(_) => "NetworkNotAllowed",
            VenaError::Db(_) => "Db",
            VenaError::Json(_) => "Json",
            VenaError::Io(_) => "Io",
            VenaError::Zip(_) => "Zip",
            VenaError::Inference(_) => "Inference",
            VenaError::Other(_) => "Other",
        }
    }
}

impl serde::Serialize for VenaError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("VenaError", 2)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type Result<T> = std::result::Result<T, VenaError>;

#[cfg(test)]
mod tests {
    use super::VenaError;

    #[test]
    fn codes_are_stable_and_message_renders() {
        assert_eq!(VenaError::NotFound("x".into()).code(), "NotFound");
        assert_eq!(VenaError::NoBackend.code(), "NoBackend");
        assert_eq!(
            VenaError::NetworkNotAllowed("h".into()).code(),
            "NetworkNotAllowed"
        );
        assert_eq!(
            VenaError::SpoilerConsentRequired.code(),
            "SpoilerConsentRequired"
        );
        assert!(VenaError::Other("boom".into()).to_string().contains("boom"));
    }

    #[test]
    fn io_error_converts_and_keeps_code() {
        let e: VenaError = std::io::Error::new(std::io::ErrorKind::NotFound, "nope").into();
        assert_eq!(e.code(), "Io");
        assert!(e.to_string().contains("nope"));
    }

    #[test]
    fn remaining_codes_and_from_conversions() {
        assert_eq!(
            VenaError::InvalidPackage("p".into()).code(),
            "InvalidPackage"
        );
        assert_eq!(VenaError::Inference("i".into()).code(), "Inference");
        // From<serde_json::Error>
        let je: VenaError = serde_json::from_str::<i32>("not json").unwrap_err().into();
        assert_eq!(je.code(), "Json");
        // From<rusqlite::Error>
        let de: VenaError = rusqlite::Error::QueryReturnedNoRows.into();
        assert_eq!(de.code(), "Db");
        // From<zip::result::ZipError>
        let ze: VenaError = zip::result::ZipError::FileNotFound.into();
        assert_eq!(ze.code(), "Zip");
    }

    #[test]
    fn serializes_to_code_and_message() {
        let json = serde_json::to_value(VenaError::NoBackend).unwrap();
        assert_eq!(json["code"], "NoBackend");
        assert!(json["message"].as_str().unwrap().contains("backend"));
    }
}
