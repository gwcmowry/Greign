//! Versioned public RAW corpus manifest types.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The only manifest schema accepted by this build.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// A public RAW corpus manifest.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusManifest {
    /// Manifest format version.
    pub schema_version: u32,
    /// Files in this corpus.
    pub entries: Vec<CorpusEntry>,
}

/// One downloadable RAW test file and its expected identity.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusEntry {
    /// Stable, filesystem-safe entry identifier.
    pub id: String,
    /// HTTPS download location.
    pub url: String,
    /// SPDX expression or a precise redistributable-use declaration.
    pub license: String,
    /// Optional page containing the full license terms.
    pub license_url: Option<String>,
    /// Lowercase hexadecimal SHA-256 digest.
    pub sha256: String,
    /// Camera manufacturer.
    pub camera_make: String,
    /// Camera model.
    pub camera_model: String,
    /// Uppercase RAW format name.
    pub format: RawFormat,
    /// Optional camera recording mode, such as C-RAW.
    pub mode: Option<String>,
    /// Expected pixel width.
    pub width: u32,
    /// Expected pixel height.
    pub height: u32,
    /// Golden decoder properties for this exact file.
    #[serde(default)]
    pub expected: ExpectedProperties,
}

/// Optional golden values checked when the corresponding decoder path runs.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedProperties {
    /// Active area as `[x, y, width, height]`.
    pub active_area: Option<[u32; 4]>,
    /// Exact CFA pattern or a descriptive X-Trans label.
    pub cfa: Option<String>,
    /// Embedded preview width.
    pub preview_width: Option<u32>,
    /// Embedded preview height.
    pub preview_height: Option<u32>,
    /// SHA-256 over decoded `u16` samples serialized little-endian.
    pub mosaic_sha256_le_u16: Option<String>,
}

/// RAW formats in the version-one compatibility matrix.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum RawFormat {
    /// Canon CR3.
    #[serde(rename = "CR3")]
    Cr3,
    /// Canon CR2.
    #[serde(rename = "CR2")]
    Cr2,
    /// Nikon NEF.
    #[serde(rename = "NEF")]
    Nef,
    /// Sony ARW.
    #[serde(rename = "ARW")]
    Arw,
    /// Fujifilm RAF.
    #[serde(rename = "RAF")]
    Raf,
    /// Panasonic RW2.
    #[serde(rename = "RW2")]
    Rw2,
    /// Olympus ORF.
    #[serde(rename = "ORF")]
    Orf,
    /// Adobe Digital Negative.
    #[serde(rename = "DNG")]
    Dng,
}

impl RawFormat {
    /// Lowercase filename extension used by local corpus storage.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Cr3 => "cr3",
            Self::Cr2 => "cr2",
            Self::Nef => "nef",
            Self::Arw => "arw",
            Self::Raf => "raf",
            Self::Rw2 => "rw2",
            Self::Orf => "orf",
            Self::Dng => "dng",
        }
    }
}

/// Failures while loading a corpus manifest.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// The manifest could not be read.
    #[error("could not read corpus manifest {path}: {source}")]
    Read {
        /// Path which failed.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The JSON representation is invalid.
    #[error("invalid corpus manifest {path}: {source}")]
    Json {
        /// Path which failed.
        path: String,
        /// Underlying JSON error.
        source: serde_json::Error,
    },
    /// The manifest uses an unsupported schema version.
    #[error("unsupported corpus manifest schema {found}; expected {expected}")]
    Schema {
        /// Encountered version.
        found: u32,
        /// Supported version.
        expected: u32,
    },
}

impl CorpusManifest {
    /// Loads a manifest while enforcing its schema version.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the file cannot be read, its JSON shape is
    /// invalid, or its schema version is unsupported.
    pub fn from_path(path: &Path) -> Result<Self, ManifestError> {
        let bytes = std::fs::read(path).map_err(|source| ManifestError::Read {
            path: path.display().to_string(),
            source,
        })?;
        let manifest: Self =
            serde_json::from_slice(&bytes).map_err(|source| ManifestError::Json {
                path: path.display().to_string(),
                source,
            })?;
        if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(ManifestError::Schema {
                found: manifest.schema_version,
                expected: MANIFEST_SCHEMA_VERSION,
            });
        }
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::{CorpusManifest, MANIFEST_SCHEMA_VERSION, ManifestError};
    use std::path::Path;

    #[test]
    fn committed_empty_manifest_parses() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/manifests/empty.json");
        let manifest = CorpusManifest::from_path(&path).expect("empty manifest should parse");
        assert_eq!(manifest.schema_version, MANIFEST_SCHEMA_VERSION);
        assert!(manifest.entries.is_empty());
    }

    #[test]
    fn unsupported_schema_is_explicit() {
        let manifest =
            serde_json::from_str::<CorpusManifest>(r#"{"schema_version":99,"entries":[]}"#)
                .expect("shape is valid");
        let error = if manifest.schema_version == MANIFEST_SCHEMA_VERSION {
            panic!("test fixture unexpectedly uses supported schema")
        } else {
            ManifestError::Schema {
                found: manifest.schema_version,
                expected: MANIFEST_SCHEMA_VERSION,
            }
        };
        assert!(
            error
                .to_string()
                .contains("unsupported corpus manifest schema 99")
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let error = serde_json::from_str::<CorpusManifest>(
            r#"{"schema_version":1,"entries":[],"surprise":true}"#,
        )
        .expect_err("unknown field should fail");
        assert!(error.to_string().contains("unknown field"));
    }
}
