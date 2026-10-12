//! Portable XMP sidecar mapping and atomic replacement.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use greign_core::EditRecipe;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;

const BEGIN: &str = "<!-- greign:begin -->";
const END: &str = "<!-- greign:end -->";
const PAYLOAD_PREFIX: &str = "greign:Payload=\"";
const GREIGN_NAMESPACE: &str = "https://greign.org/ns/1.0/";

/// Portable virtual-copy representation stored in a sidecar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VirtualCopySidecar {
    /// User-visible copy name.
    pub name: String,
    /// Independent canonical recipe.
    pub recipe: EditRecipe,
}

/// Standard metadata plus Greign edits persisted in XMP.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarPayload {
    /// XMP rating from -1 (rejected) through 5.
    pub rating: Option<i8>,
    /// XMP label.
    pub label: Option<String>,
    /// Greign flag state from -1 through 1.
    pub flag: Option<i8>,
    /// Dublin Core subject keywords.
    pub keywords: Vec<String>,
    /// Base edit recipe.
    pub recipe: EditRecipe,
    /// Independent virtual copies.
    pub virtual_copies: Vec<VirtualCopySidecar>,
}

/// SHA-256 identity used for optimistic sidecar conflict detection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SidecarSnapshot([u8; 32]);

impl SidecarSnapshot {
    /// Snapshot digest bytes.
    #[must_use]
    pub const fn digest(self) -> [u8; 32] {
        self.0
    }
}

/// Reads Greign data from an XMP sidecar and records its exact byte identity.
///
/// Unknown XML elements, attributes, and namespaces are not interpreted. They
/// remain in the original document and are preserved by the writer.
///
/// # Errors
///
/// Returns [`SidecarError`] for I/O, malformed owned markup, invalid fields,
/// or recipe/JSON failures.
pub fn read_sidecar(path: &Path) -> Result<(SidecarPayload, SidecarSnapshot), SidecarError> {
    let bytes = fs::read(path)?;
    let snapshot = snapshot(&bytes);
    let xml = std::str::from_utf8(&bytes).map_err(|_| SidecarError::Malformed)?;
    let payload = extract_payload(xml)?.unwrap_or_default();
    validate_payload(&payload)?;
    Ok((payload, snapshot))
}

/// Atomically rewrites Greign's owned XMP block while retaining all unknown
/// XML outside that block byte-for-byte.
///
/// When `expected` is supplied, a changed or newly created sidecar is rejected
/// as a conflict. The temporary file is flushed and synced before same-folder
/// atomic rename, then the parent directory is synced.
///
/// # Errors
///
/// Returns [`SidecarError`] for conflicts, invalid payloads, malformed XMP,
/// permissions, disk/filesystem failures, or serialization errors.
pub fn write_sidecar_atomic(
    path: &Path,
    payload: &SidecarPayload,
    expected: Option<SidecarSnapshot>,
) -> Result<SidecarSnapshot, SidecarError> {
    write_sidecar_impl(path, payload, expected, false)
}

fn write_sidecar_impl(
    path: &Path,
    payload: &SidecarPayload,
    expected: Option<SidecarSnapshot>,
    inject_failure_before_persist: bool,
) -> Result<SidecarSnapshot, SidecarError> {
    validate_payload(payload)?;
    let existing = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    if expected.is_some_and(|expected| expected != snapshot(&existing)) {
        return Err(SidecarError::Conflict);
    }
    let existing_xml = std::str::from_utf8(&existing).map_err(|_| SidecarError::Malformed)?;
    let base = remove_owned_block(existing_xml)?;
    let block = owned_block(payload)?;
    let document = if base.is_empty() {
        format!(
            "<?xpacket begin=\"﻿\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
             {block}\n</rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>\n"
        )
    } else {
        let insertion = base.rfind("</rdf:RDF>").ok_or(SidecarError::Malformed)?;
        let mut document = String::with_capacity(base.len() + block.len() + 1);
        document.push_str(&base[..insertion]);
        document.push_str(&block);
        document.push('\n');
        document.push_str(&base[insertion..]);
        document
    };
    let parent = path.parent().ok_or(SidecarError::MissingParent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(document.as_bytes())?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    if inject_failure_before_persist {
        return Err(SidecarError::Io(std::io::Error::other(
            "injected storage failure before atomic replacement",
        )));
    }
    temporary
        .persist(path)
        .map_err(|error| SidecarError::Io(error.error))?;
    File::open(parent)?.sync_all()?;
    Ok(snapshot(document.as_bytes()))
}

fn owned_block(payload: &SidecarPayload) -> Result<String, SidecarError> {
    let json = serde_json::to_string(payload)?;
    let rating = payload
        .rating
        .map(|rating| format!(" xmp:Rating=\"{rating}\""))
        .unwrap_or_default();
    let label = payload
        .label
        .as_deref()
        .map(|label| format!(" xmp:Label=\"{}\"", escape_xml(label)))
        .unwrap_or_default();
    let mut keywords = String::new();
    for keyword in &payload.keywords {
        let _ = write!(keywords, "<rdf:li>{}</rdf:li>", escape_xml(keyword));
    }
    Ok(format!(
        "{BEGIN}\n<rdf:Description xmlns:greign=\"{GREIGN_NAMESPACE}\" \
         xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\"{rating}{label} \
         greign:Payload=\"{}\"><dc:subject><rdf:Bag>{keywords}</rdf:Bag></dc:subject>\
         </rdf:Description>\n{END}",
        escape_xml(&json)
    ))
}

fn extract_payload(xml: &str) -> Result<Option<SidecarPayload>, SidecarError> {
    let Some(begin) = xml.find(BEGIN) else {
        return Ok(None);
    };
    let end = xml[begin..]
        .find(END)
        .map(|offset| begin + offset)
        .ok_or(SidecarError::Malformed)?;
    let owned = &xml[begin..end];
    let payload_start = owned
        .find(PAYLOAD_PREFIX)
        .map(|offset| offset + PAYLOAD_PREFIX.len())
        .ok_or(SidecarError::Malformed)?;
    let payload_end = owned[payload_start..]
        .find('"')
        .map(|offset| payload_start + offset)
        .ok_or(SidecarError::Malformed)?;
    let json = unescape_xml(&owned[payload_start..payload_end])?;
    Ok(Some(serde_json::from_str(&json)?))
}

fn remove_owned_block(xml: &str) -> Result<String, SidecarError> {
    let Some(begin) = xml.find(BEGIN) else {
        return Ok(xml.to_owned());
    };
    let end = xml[begin..]
        .find(END)
        .map(|offset| begin + offset + END.len())
        .ok_or(SidecarError::Malformed)?;
    let mut output = String::with_capacity(xml.len());
    output.push_str(&xml[..begin]);
    output.push_str(&xml[end..]);
    Ok(output)
}

fn validate_payload(payload: &SidecarPayload) -> Result<(), SidecarError> {
    if payload
        .rating
        .is_some_and(|rating| !(-1..=5).contains(&rating))
        || payload.flag.is_some_and(|flag| !(-1..=1).contains(&flag))
    {
        return Err(SidecarError::InvalidMetadata);
    }
    payload.recipe.to_canonical_json()?;
    for copy in &payload.virtual_copies {
        copy.recipe.to_canonical_json()?;
    }
    Ok(())
}

fn snapshot(bytes: &[u8]) -> SidecarSnapshot {
    SidecarSnapshot(Sha256::digest(bytes).into())
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
}

fn unescape_xml(value: &str) -> Result<String, SidecarError> {
    let unescaped = value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    if unescaped.contains("&#") {
        return Err(SidecarError::Malformed);
    }
    Ok(unescaped)
}

/// Sidecar persistence failure.
#[derive(Debug, Error)]
pub enum SidecarError {
    /// Filesystem operation failed.
    #[error("XMP sidecar I/O failure: {0}")]
    Io(#[from] std::io::Error),
    /// Greign payload JSON is malformed.
    #[error("XMP sidecar payload failure: {0}")]
    Json(#[from] serde_json::Error),
    /// Recipe validation failed.
    #[error("XMP sidecar recipe failure: {0}")]
    Recipe(#[from] greign_core::RecipeError),
    /// Sidecar changed since it was read.
    #[error("XMP sidecar changed on disk")]
    Conflict,
    /// Owned marker block or enclosing RDF is malformed.
    #[error("XMP sidecar structure is malformed")]
    Malformed,
    /// Rating or flag is outside its documented domain.
    #[error("XMP rating or flag is invalid")]
    InvalidMetadata,
    /// Sidecar path has no parent directory.
    #[error("XMP sidecar path has no parent directory")]
    MissingParent,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        SidecarError, SidecarPayload, VirtualCopySidecar, read_sidecar, write_sidecar_atomic,
    };
    use greign_core::{EditRecipe, OperatorId, OperatorParameters};

    fn edited_recipe(ev: f32) -> EditRecipe {
        let mut recipe = EditRecipe::default();
        recipe
            .operator_mut(OperatorId::Exposure)
            .expect("exposure")
            .parameters = OperatorParameters::Exposure { ev };
        recipe
    }

    #[test]
    fn unknown_xmp_and_namespaces_survive_owned_block_rewrite() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("photo.xmp");
        let unknown = "<rdf:Description xmlns:vendor=\"urn:vendor\" vendor:Secret=\"42\"/>";
        fs::write(
            &path,
            format!(
                "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                 xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                 {unknown}</rdf:RDF></x:xmpmeta>"
            ),
        )
        .expect("fixture");
        let before = fs::read(&path).expect("fixture");
        let expected = super::snapshot(&before);
        let payload = SidecarPayload {
            rating: Some(5),
            label: Some("Blue & Gold".into()),
            flag: Some(1),
            keywords: vec!["family".into(), "R&D".into()],
            recipe: edited_recipe(1.0),
            virtual_copies: vec![VirtualCopySidecar {
                name: "bright".into(),
                recipe: edited_recipe(2.0),
            }],
        };
        write_sidecar_atomic(&path, &payload, Some(expected)).expect("write");
        let xml = fs::read_to_string(&path).expect("sidecar");
        assert!(xml.contains(unknown));
        assert!(xml.contains("xmp:Rating=\"5\""));
        assert!(xml.contains("<rdf:li>R&amp;D</rdf:li>"));
        let (read, _) = read_sidecar(&path).expect("read");
        assert_eq!(read, payload);
    }

    #[test]
    fn stale_snapshot_conflict_leaves_current_file_untouched() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("photo.xmp");
        let first = write_sidecar_atomic(&path, &SidecarPayload::default(), None).expect("write");
        fs::write(&path, b"external replacement").expect("external write");
        let result = write_sidecar_atomic(&path, &SidecarPayload::default(), Some(first));
        assert!(matches!(result, Err(SidecarError::Conflict)));
        assert_eq!(fs::read(&path).expect("current"), b"external replacement");
    }

    #[test]
    fn restart_round_trip_preserves_edits_and_virtual_copies() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("photo.xmp");
        let payload = SidecarPayload {
            recipe: edited_recipe(-0.75),
            virtual_copies: vec![VirtualCopySidecar {
                name: "alternate".into(),
                recipe: edited_recipe(0.5),
            }],
            ..SidecarPayload::default()
        };
        let snapshot = write_sidecar_atomic(&path, &payload, None).expect("write");
        let (reopened, reopened_snapshot) = read_sidecar(&path).expect("read");
        assert_eq!(reopened, payload);
        assert_eq!(reopened_snapshot, snapshot);
    }

    #[test]
    fn injected_storage_failure_before_rename_preserves_previous_sidecar() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("photo.xmp");
        write_sidecar_atomic(&path, &SidecarPayload::default(), None).expect("initial");
        let previous = fs::read(&path).expect("previous");
        let result = super::write_sidecar_impl(
            &path,
            &SidecarPayload {
                rating: Some(5),
                ..SidecarPayload::default()
            },
            None,
            true,
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).expect("preserved"), previous);
    }

    #[cfg(unix)]
    #[test]
    fn read_only_directory_reports_error_without_touching_existing_sidecar() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("photo.xmp");
        write_sidecar_atomic(&path, &SidecarPayload::default(), None).expect("initial");
        let previous = fs::read(&path).expect("previous");
        let original_mode = fs::metadata(directory.path())
            .expect("metadata")
            .permissions()
            .mode();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o500))
            .expect("read only");
        let result = write_sidecar_atomic(
            &path,
            &SidecarPayload {
                rating: Some(4),
                ..SidecarPayload::default()
            },
            None,
        );
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(original_mode))
            .expect("restore permissions");
        assert!(result.is_err());
        assert_eq!(fs::read(&path).expect("preserved"), previous);
    }
}
