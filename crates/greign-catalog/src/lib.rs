//! Durable `SQLite` catalog and portable XMP sidecars.

mod catalog;
mod sidecar;

pub use catalog::{AssetId, Catalog, CatalogError, RebuildSource, VirtualCopy, VirtualCopyId};
pub use sidecar::{
    SidecarError, SidecarPayload, SidecarSnapshot, VirtualCopySidecar, read_sidecar,
    write_sidecar_atomic,
};
