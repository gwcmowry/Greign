//! Corpus integrity and decoder-expectation verification.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::RawDecoder;
use crate::manifest::{CorpusEntry, CorpusManifest, ManifestError};

/// Summary of a corpus verification run.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct VerificationReport {
    /// Entries declared by the manifest.
    pub declared: usize,
    /// Files whose digest and optional metadata matched.
    pub verified: usize,
    /// Missing files tolerated by the caller.
    pub missing: usize,
    /// Full mosaics checked against a declared digest.
    pub mosaics_verified: usize,
}

/// Corpus manifest or file failures.
#[derive(Debug, Error)]
pub enum CorpusError {
    /// Manifest loading failed.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// A manifest invariant failed.
    #[error("invalid corpus entry {id}: {reason}")]
    Entry {
        /// Entry identifier.
        id: String,
        /// Violated invariant.
        reason: String,
    },
    /// A required corpus file is missing.
    #[error("corpus file is missing for {id}: {path}")]
    Missing {
        /// Entry identifier.
        id: String,
        /// Expected local path.
        path: String,
    },
    /// A corpus file could not be read.
    #[error("could not read corpus file for {id} at {path}: {source}")]
    Io {
        /// Entry identifier.
        id: String,
        /// Local path.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Content does not have its declared digest.
    #[error("SHA-256 mismatch for {id}: expected {expected}, found {actual}")]
    Digest {
        /// Entry identifier.
        id: String,
        /// Declared digest.
        expected: String,
        /// Measured digest.
        actual: String,
    },
    /// Decoder metadata extraction failed.
    #[error("decoder rejected corpus entry {id}: {reason}")]
    Decode {
        /// Entry identifier.
        id: String,
        /// Backend error.
        reason: String,
    },
}

/// Loads and verifies corpus declarations and any locally available files.
///
/// When `allow_missing` is true, absent files are counted rather than treated
/// as errors. Passing a decoder additionally checks metadata and previews.
/// `verify_mosaics` enables the more expensive full-decode digest checks.
///
/// # Errors
///
/// Returns [`CorpusError`] on malformed declarations, file integrity failures,
/// missing required files, or decoder expectation mismatches.
pub fn verify(
    manifest_path: &Path,
    root: &Path,
    allow_missing: bool,
    decoder: Option<&dyn RawDecoder>,
    verify_mosaics: bool,
) -> Result<VerificationReport, CorpusError> {
    let manifest = CorpusManifest::from_path(manifest_path)?;
    validate_manifest(&manifest)?;
    let mut report = VerificationReport {
        declared: manifest.entries.len(),
        ..VerificationReport::default()
    };
    for entry in &manifest.entries {
        let path = entry_path(root, entry);
        if !path.is_file() {
            if allow_missing {
                report.missing += 1;
                continue;
            }
            return Err(CorpusError::Missing {
                id: entry.id.clone(),
                path: path.display().to_string(),
            });
        }
        verify_digest(entry, &path)?;
        if let Some(decoder) = decoder {
            verify_metadata(entry, &path, decoder)?;
            verify_preview(entry, &path, decoder)?;
            if verify_mosaics && entry.expected.mosaic_sha256_le_u16.is_some() {
                verify_mosaic(entry, &path, decoder)?;
                report.mosaics_verified += 1;
            }
        }
        report.verified += 1;
    }
    Ok(report)
}

fn validate_manifest(manifest: &CorpusManifest) -> Result<(), CorpusError> {
    let mut ids = HashSet::new();
    for entry in &manifest.entries {
        let valid_id = !entry.id.is_empty()
            && entry.id.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
            });
        require(entry, valid_id, "id is not lowercase and filesystem-safe")?;
        require(entry, ids.insert(&entry.id), "id is duplicated")?;
        require(
            entry,
            entry.url.starts_with("https://"),
            "URL must use HTTPS",
        )?;
        require(
            entry,
            !entry.license.trim().is_empty() && !entry.license.eq_ignore_ascii_case("unknown"),
            "license declaration is missing or unknown",
        )?;
        require(
            entry,
            entry.sha256.len() == 64
                && entry
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "SHA-256 must be 64 lowercase hexadecimal characters",
        )?;
        require(
            entry,
            entry.width > 0 && entry.height > 0,
            "dimensions must be nonzero",
        )?;
    }
    Ok(())
}

fn require(entry: &CorpusEntry, condition: bool, reason: &str) -> Result<(), CorpusError> {
    if condition {
        Ok(())
    } else {
        Err(CorpusError::Entry {
            id: entry.id.clone(),
            reason: reason.into(),
        })
    }
}

fn entry_path(root: &Path, entry: &CorpusEntry) -> PathBuf {
    root.join(format!("{}.{}", entry.id, entry.format.extension()))
}

fn verify_digest(entry: &CorpusEntry, path: &Path) -> Result<(), CorpusError> {
    let file = File::open(path).map_err(|source| CorpusError::Io {
        id: entry.id.clone(),
        path: path.display().to_string(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024].into_boxed_slice();
    loop {
        let read = reader.read(&mut buffer).map_err(|source| CorpusError::Io {
            id: entry.id.clone(),
            path: path.display().to_string(),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != entry.sha256 {
        return Err(CorpusError::Digest {
            id: entry.id.clone(),
            expected: entry.sha256.clone(),
            actual,
        });
    }
    Ok(())
}

fn verify_metadata(
    entry: &CorpusEntry,
    path: &Path,
    decoder: &dyn RawDecoder,
) -> Result<(), CorpusError> {
    let metadata = decoder
        .metadata(path)
        .map_err(|error| CorpusError::Decode {
            id: entry.id.clone(),
            reason: error.to_string(),
        })?;
    require(
        entry,
        metadata.width == entry.width,
        "decoded width differs from manifest",
    )?;
    require(
        entry,
        metadata.height == entry.height,
        "decoded height differs from manifest",
    )?;
    require(
        entry,
        metadata
            .make
            .trim()
            .eq_ignore_ascii_case(entry.camera_make.trim()),
        "decoded camera make differs from manifest",
    )?;
    require(
        entry,
        metadata
            .model
            .trim()
            .eq_ignore_ascii_case(entry.camera_model.trim()),
        "decoded camera model differs from manifest",
    )?;
    if let Some(mode) = &entry.mode {
        require(
            entry,
            metadata.mode.trim().eq_ignore_ascii_case(mode.trim()),
            "decoded recording mode differs from manifest",
        )?;
    }
    if let Some(cfa) = &entry.expected.cfa {
        let matches = if cfa == "x-trans-6x6" {
            metadata
                .cfa_pattern
                .as_ref()
                .is_some_and(|pattern| pattern.len() == 36)
        } else {
            metadata.cfa_pattern.as_deref() == Some(cfa.as_str())
        };
        require(entry, matches, "decoded CFA differs from manifest")?;
    }
    if let Some(expected) = entry.expected.active_area {
        let actual = metadata
            .active_area
            .map(|area| [area.x, area.y, area.width, area.height]);
        require(
            entry,
            actual == Some(expected),
            "decoded active area differs from manifest",
        )?;
    }
    Ok(())
}

fn verify_preview(
    entry: &CorpusEntry,
    path: &Path,
    decoder: &dyn RawDecoder,
) -> Result<(), CorpusError> {
    let Some(expected_width) = entry.expected.preview_width else {
        return Ok(());
    };
    let Some(expected_height) = entry.expected.preview_height else {
        return Err(CorpusError::Entry {
            id: entry.id.clone(),
            reason: "preview width is declared without preview height".into(),
        });
    };
    let preview = decoder
        .embedded_preview(path)
        .map_err(|error| CorpusError::Decode {
            id: entry.id.clone(),
            reason: error.to_string(),
        })?;
    require(
        entry,
        preview.width == expected_width && preview.height == expected_height,
        "embedded preview dimensions differ from manifest",
    )
}

fn verify_mosaic(
    entry: &CorpusEntry,
    path: &Path,
    decoder: &dyn RawDecoder,
) -> Result<(), CorpusError> {
    let mosaic = decoder.mosaic(path).map_err(|error| CorpusError::Decode {
        id: entry.id.clone(),
        reason: error.to_string(),
    })?;
    let mut hasher = Sha256::new();
    for sample in mosaic.samples {
        hasher.update(sample.to_le_bytes());
    }
    let actual = format!("{:x}", hasher.finalize());
    let expected = entry
        .expected
        .mosaic_sha256_le_u16
        .as_ref()
        .expect("caller checks the expected digest");
    require(
        entry,
        &actual == expected,
        "decoded mosaic digest differs from manifest",
    )
}

#[cfg(test)]
mod tests {
    use super::{CorpusError, verify};
    use std::fs;

    #[test]
    fn empty_committed_manifest_verifies_without_data() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/manifests/empty.json");
        let root = tempfile::tempdir().expect("create corpus root");
        let report =
            verify(&manifest, root.path(), false, None, false).expect("verify empty corpus");
        assert_eq!(report.declared, 0);
        assert_eq!(report.verified, 0);
    }

    #[test]
    fn digest_mismatch_is_reported() {
        let root = tempfile::tempdir().expect("create corpus root");
        let manifest = root.path().join("manifest.json");
        fs::write(
            &manifest,
            r#"{
                "schema_version": 1,
                "entries": [{
                    "id": "sample",
                    "url": "https://example.invalid/sample.dng",
                    "license": "CC0-1.0",
                    "license_url": null,
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "camera_make": "Test",
                    "camera_model": "Fixture",
                    "format": "DNG",
                    "mode": null,
                    "width": 1,
                    "height": 1
                }]
            }"#,
        )
        .expect("write manifest");
        fs::write(root.path().join("sample.dng"), b"not the declared bytes")
            .expect("write corpus file");
        assert!(matches!(
            verify(&manifest, root.path(), false, None, false),
            Err(CorpusError::Digest { .. })
        ));
    }

    #[test]
    fn missing_files_can_be_explicitly_skipped() {
        let root = tempfile::tempdir().expect("create corpus root");
        let manifest = root.path().join("manifest.json");
        fs::write(
            &manifest,
            r#"{
                "schema_version": 1,
                "entries": [{
                    "id": "missing",
                    "url": "https://example.invalid/missing.cr3",
                    "license": "CC0-1.0",
                    "license_url": null,
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "camera_make": "Canon",
                    "camera_model": "EOS R7",
                    "format": "CR3",
                    "mode": null,
                    "width": 1,
                    "height": 1
                }]
            }"#,
        )
        .expect("write manifest");
        let report = verify(&manifest, root.path(), true, None, false).expect("allow missing");
        assert_eq!(report.missing, 1);
    }
}
