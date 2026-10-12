//! `SQLite` catalog schema and transactional workflows.

use std::path::{Path, PathBuf};
use std::time::Duration;

use greign_core::{CATALOG_SCHEMA_VERSION, EditRecipe, RecipeError};
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

use crate::sidecar::{SidecarError, read_sidecar};

/// Stable catalog asset row identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AssetId(pub i64);

/// Stable virtual-copy row identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct VirtualCopyId(pub i64);

/// One non-destructive virtual copy.
#[derive(Clone, Debug, PartialEq)]
pub struct VirtualCopy {
    /// Stable row identifier.
    pub id: VirtualCopyId,
    /// Original asset shared by this copy.
    pub asset_id: AssetId,
    /// User-visible name.
    pub name: String,
    /// Independent edit recipe.
    pub recipe: EditRecipe,
}

/// Source and portable sidecar inputs used to reconstruct a fresh catalog.
#[derive(Clone, Debug)]
pub struct RebuildSource {
    /// Immutable original path.
    pub source_path: PathBuf,
    /// Precomputed source content identity.
    pub content_hash: [u8; 32],
    /// Original byte length.
    pub file_size: u64,
    /// Original modification timestamp in nanoseconds.
    pub modified_ns: i64,
    /// Decoder metadata JSON.
    pub metadata_json: String,
    /// Optional portable sidecar path.
    pub sidecar_path: Option<PathBuf>,
}

/// Open migrated catalog connection.
#[derive(Debug)]
pub struct Catalog {
    connection: Connection,
    path: PathBuf,
}

impl Catalog {
    /// Opens or creates a catalog, enables WAL/foreign keys, and migrates it.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for filesystem, `SQLite`, or migration failures.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CatalogError> {
        let path = path.as_ref().to_path_buf();
        let mut connection = Connection::open(&path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&mut connection)?;
        Ok(Self { connection, path })
    }

    /// Filesystem path backing this catalog.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Imports a source identity without opening the original for writing.
    ///
    /// Duplicate content reuses the existing asset and records an additional
    /// path identity.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for invalid sizes, paths, JSON, or `SQLite`
    /// transaction failures.
    pub fn import_asset(
        &mut self,
        path: &Path,
        content_hash: [u8; 32],
        file_size: u64,
        modified_ns: i64,
        metadata_json: &str,
    ) -> Result<AssetId, CatalogError> {
        serde_json::from_str::<serde_json::Value>(metadata_json)?;
        let path = path.to_str().ok_or(CatalogError::NonUtf8Path)?.to_owned();
        let file_size = i64::try_from(file_size).map_err(|_| CatalogError::FileTooLarge)?;
        let transaction = self.connection.transaction()?;
        let existing = transaction
            .query_row(
                "SELECT id FROM assets WHERE content_hash = ?1",
                [content_hash.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        let asset_id = if let Some(id) = existing {
            id
        } else {
            transaction.execute(
                "INSERT INTO assets(content_hash, current_path, file_size, modified_ns) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![content_hash.as_slice(), path, file_size, modified_ns],
            )?;
            transaction.last_insert_rowid()
        };
        transaction.execute(
            "INSERT INTO file_identities(asset_id, path, missing) VALUES (?1, ?2, 0) \
             ON CONFLICT(path) DO UPDATE SET asset_id=excluded.asset_id, missing=0",
            params![asset_id, path],
        )?;
        transaction.execute(
            "UPDATE assets SET current_path=?2, file_size=?3, modified_ns=?4 WHERE id=?1",
            params![asset_id, path, file_size, modified_ns],
        )?;
        transaction.execute(
            "INSERT INTO metadata(asset_id, json) VALUES (?1, ?2) \
             ON CONFLICT(asset_id) DO UPDATE SET json=excluded.json",
            params![asset_id, metadata_json],
        )?;
        transaction.commit()?;
        Ok(AssetId(asset_id))
    }

    /// Marks every recorded path for an asset missing.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for `SQLite` failures.
    pub fn mark_missing(&mut self, asset_id: AssetId) -> Result<(), CatalogError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE file_identities SET missing=1 WHERE asset_id=?1",
            [asset_id.0],
        )?;
        transaction.execute(
            "UPDATE assets SET current_path=NULL WHERE id=?1",
            [asset_id.0],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Associates a moved original with its existing content identity.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for non-UTF-8 paths or `SQLite` failures.
    pub fn relink(&mut self, asset_id: AssetId, path: &Path) -> Result<(), CatalogError> {
        let path = path.to_str().ok_or(CatalogError::NonUtf8Path)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO file_identities(asset_id, path, missing) VALUES (?1, ?2, 0) \
             ON CONFLICT(path) DO UPDATE SET asset_id=excluded.asset_id, missing=0",
            params![asset_id.0, path],
        )?;
        transaction.execute(
            "UPDATE assets SET current_path=?2 WHERE id=?1",
            params![asset_id.0, path],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Current original path, or `None` when missing.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for `SQLite` failures or an unknown asset.
    pub fn current_path(&self, asset_id: AssetId) -> Result<Option<PathBuf>, CatalogError> {
        let path: Option<String> = self.connection.query_row(
            "SELECT current_path FROM assets WHERE id=?1",
            [asset_id.0],
            |row| row.get(0),
        )?;
        Ok(path.map(PathBuf::from))
    }

    /// Persists the base recipe transactionally.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for invalid recipes, unknown assets, or `SQLite`
    /// failures.
    pub fn save_recipe(
        &mut self,
        asset_id: AssetId,
        recipe: &EditRecipe,
    ) -> Result<(), CatalogError> {
        let json = recipe.to_canonical_json()?;
        self.connection.execute(
            "INSERT INTO recipes(asset_id, recipe_json) VALUES (?1, ?2) \
             ON CONFLICT(asset_id) DO UPDATE SET recipe_json=excluded.recipe_json",
            params![asset_id.0, json],
        )?;
        Ok(())
    }

    /// Loads the base recipe, if one has been saved.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for `SQLite` or recipe validation failures.
    pub fn load_recipe(&self, asset_id: AssetId) -> Result<Option<EditRecipe>, CatalogError> {
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT recipe_json FROM recipes WHERE asset_id=?1",
                [asset_id.0],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|json| EditRecipe::from_json(&json).map_err(CatalogError::from))
            .transpose()
    }

    /// Creates a virtual copy with an independent recipe.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for invalid recipes, unknown assets, or `SQLite`
    /// failures.
    pub fn create_virtual_copy(
        &mut self,
        asset_id: AssetId,
        name: &str,
        recipe: &EditRecipe,
    ) -> Result<VirtualCopyId, CatalogError> {
        let json = recipe.to_canonical_json()?;
        self.connection.execute(
            "INSERT INTO virtual_copies(asset_id, name, recipe_json) VALUES (?1, ?2, ?3)",
            params![asset_id.0, name, json],
        )?;
        Ok(VirtualCopyId(self.connection.last_insert_rowid()))
    }

    /// Lists virtual copies in stable row order.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for `SQLite` or recipe validation failures.
    pub fn virtual_copies(&self, asset_id: AssetId) -> Result<Vec<VirtualCopy>, CatalogError> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, recipe_json FROM virtual_copies WHERE asset_id=?1 ORDER BY id",
        )?;
        let rows = statement.query_map([asset_id.0], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, json) = row?;
            Ok(VirtualCopy {
                id: VirtualCopyId(id),
                asset_id,
                name,
                recipe: EditRecipe::from_json(&json)?,
            })
        })
        .collect()
    }

    /// Runs `PRAGMA integrity_check` and requires an `ok` result.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for `SQLite` failures or detected corruption.
    pub fn integrity_check(&self) -> Result<(), CatalogError> {
        let result: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if result == "ok" {
            Ok(())
        } else {
            Err(CatalogError::Integrity(result))
        }
    }

    /// Produces a consistent online backup.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for destination or backup failures.
    pub fn backup(&self, destination: &Path) -> Result<(), CatalogError> {
        let mut destination = Connection::open(destination)?;
        let backup = rusqlite::backup::Backup::new(&self.connection, &mut destination)?;
        backup.run_to_completion(64, Duration::from_millis(5), None)?;
        Ok(())
    }

    /// Rebuilds asset, recipe, and virtual-copy rows from immutable sources
    /// and their portable sidecars.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for source metadata, sidecar, recipe, or
    /// transactional catalog failures.
    pub fn rebuild_sources(
        &mut self,
        sources: &[RebuildSource],
    ) -> Result<Vec<AssetId>, CatalogError> {
        let mut assets = Vec::with_capacity(sources.len());
        for source in sources {
            let asset = self.import_asset(
                &source.source_path,
                source.content_hash,
                source.file_size,
                source.modified_ns,
                &source.metadata_json,
            )?;
            if let Some(sidecar_path) = &source.sidecar_path {
                let (payload, _) = read_sidecar(sidecar_path)?;
                self.save_recipe(asset, &payload.recipe)?;
                for copy in payload.virtual_copies {
                    self.create_virtual_copy(asset, &copy.name, &copy.recipe)?;
                }
            }
            assets.push(asset);
        }
        Ok(assets)
    }
}

fn migrate(connection: &mut Connection) -> Result<(), CatalogError> {
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > CATALOG_SCHEMA_VERSION {
        return Err(CatalogError::NewerSchema(version));
    }
    if version == 0 {
        apply_migration(connection, CATALOG_SCHEMA_VERSION, SCHEMA_V1)?;
    }
    Ok(())
}

fn apply_migration(
    connection: &mut Connection,
    version: u32,
    statements: &str,
) -> Result<(), CatalogError> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(statements)?;
    transaction.pragma_update(None, "user_version", version)?;
    transaction.commit()?;
    Ok(())
}

const SCHEMA_V1: &str = r"
CREATE TABLE assets (
    id INTEGER PRIMARY KEY,
    content_hash BLOB NOT NULL UNIQUE CHECK(length(content_hash) = 32),
    current_path TEXT,
    file_size INTEGER NOT NULL CHECK(file_size >= 0),
    modified_ns INTEGER NOT NULL
);
CREATE TABLE file_identities (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    path TEXT NOT NULL UNIQUE,
    device INTEGER,
    inode INTEGER,
    missing INTEGER NOT NULL DEFAULT 0 CHECK(missing IN (0, 1))
);
CREATE TABLE metadata (
    asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    json TEXT NOT NULL CHECK(json_valid(json))
);
CREATE TABLE recipes (
    asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    recipe_json TEXT NOT NULL CHECK(json_valid(recipe_json))
);
CREATE TABLE virtual_copies (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    recipe_json TEXT NOT NULL CHECK(json_valid(recipe_json))
);
CREATE TABLE history (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    virtual_copy_id INTEGER REFERENCES virtual_copies(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    recipe_json TEXT NOT NULL CHECK(json_valid(recipe_json)),
    UNIQUE(asset_id, virtual_copy_id, sequence)
);
CREATE TABLE collections (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
CREATE TABLE collection_members (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY(collection_id, asset_id)
);
CREATE TABLE stacks (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE stack_members (
    stack_id INTEGER NOT NULL REFERENCES stacks(id) ON DELETE CASCADE,
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY(stack_id, asset_id)
);
CREATE TABLE film_rolls (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, shared_recipe_json TEXT);
CREATE TABLE film_roll_members (
    film_roll_id INTEGER NOT NULL REFERENCES film_rolls(id) ON DELETE CASCADE,
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY(film_roll_id, asset_id)
);
CREATE TABLE cache_records (
    key BLOB PRIMARY KEY,
    asset_id INTEGER REFERENCES assets(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    bytes INTEGER NOT NULL CHECK(bytes >= 0),
    last_used_ns INTEGER NOT NULL
);
CREATE INDEX file_identities_asset ON file_identities(asset_id);
CREATE INDEX virtual_copies_asset ON virtual_copies(asset_id);
";

/// Catalog persistence failure.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// `SQLite` operation failed.
    #[error("catalog SQLite failure: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Recipe validation failed.
    #[error("catalog recipe failure: {0}")]
    Recipe(#[from] RecipeError),
    /// Metadata JSON is malformed.
    #[error("catalog metadata JSON failure: {0}")]
    Json(#[from] serde_json::Error),
    /// Source path cannot be represented in the current schema.
    #[error("catalog path is not valid UTF-8")]
    NonUtf8Path,
    /// File size exceeds `SQLite`'s signed integer domain.
    #[error("source file is too large for the catalog schema")]
    FileTooLarge,
    /// Catalog was created by a newer Greign build.
    #[error("catalog schema {0} is newer than this build")]
    NewerSchema(u32),
    /// `SQLite` reported corruption.
    #[error("catalog integrity check failed: {0}")]
    Integrity(String),
    /// Portable sidecar could not be read or validated.
    #[error("catalog sidecar failure: {0}")]
    Sidecar(#[from] SidecarError),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{AssetId, Catalog, RebuildSource, apply_migration};
    use crate::{SidecarPayload, VirtualCopySidecar, write_sidecar_atomic};
    use greign_core::{EditRecipe, OperatorId, OperatorParameters};

    #[test]
    fn duplicate_content_reuses_asset_and_relink_survives_restart() {
        let directory = tempdir().expect("tempdir");
        let database = directory.path().join("catalog.sqlite3");
        let first = directory.path().join("first.cr3");
        let moved = directory.path().join("moved.cr3");
        fs::write(&first, b"immutable raw").expect("source");
        let original = fs::read(&first).expect("source bytes");
        let mut catalog = Catalog::open(&database).expect("catalog");
        let asset = catalog
            .import_asset(&first, [7; 32], original.len() as u64, 1, r#"{"iso":100}"#)
            .expect("import");
        let duplicate = catalog
            .import_asset(&moved, [7; 32], original.len() as u64, 2, r#"{"iso":100}"#)
            .expect("duplicate");
        assert_eq!(asset, duplicate);
        catalog.mark_missing(asset).expect("missing");
        assert_eq!(catalog.current_path(asset).expect("path"), None);
        catalog.relink(asset, &moved).expect("relink");
        drop(catalog);
        let catalog = Catalog::open(&database).expect("reopen");
        assert_eq!(catalog.current_path(asset).expect("path"), Some(moved));
        assert_eq!(fs::read(&first).expect("source bytes"), original);
        catalog.integrity_check().expect("integrity");
    }

    #[test]
    fn recipes_and_virtual_copies_survive_restart_and_backup() {
        let directory = tempdir().expect("tempdir");
        let database = directory.path().join("catalog.sqlite3");
        let backup = directory.path().join("backup.sqlite3");
        let source = directory.path().join("photo.cr3");
        let mut catalog = Catalog::open(&database).expect("catalog");
        let asset = catalog
            .import_asset(&source, [8; 32], 100, 1, "{}")
            .expect("import");
        let mut recipe = EditRecipe::default();
        recipe
            .operator_mut(OperatorId::Exposure)
            .expect("exposure")
            .parameters = OperatorParameters::Exposure { ev: 1.25 };
        catalog.save_recipe(asset, &recipe).expect("recipe");
        catalog
            .create_virtual_copy(asset, "brighter", &recipe)
            .expect("copy");
        catalog.backup(&backup).expect("backup");
        drop(catalog);

        for path in [&database, &backup] {
            let catalog = Catalog::open(path).expect("reopen");
            assert_eq!(
                catalog.load_recipe(asset).expect("recipe"),
                Some(recipe.clone())
            );
            assert_eq!(catalog.virtual_copies(asset).expect("copies").len(), 1);
            catalog.integrity_check().expect("integrity");
        }
    }

    #[test]
    fn foreign_keys_and_uncommitted_writes_survive_injected_failure() {
        let directory = tempdir().expect("tempdir");
        let database = directory.path().join("catalog.sqlite3");
        let mut catalog = Catalog::open(&database).expect("catalog");
        assert!(catalog.current_path(AssetId(999)).is_err());
        {
            let transaction = catalog.connection.transaction().expect("transaction");
            transaction
                .execute(
                    "INSERT INTO assets(content_hash, current_path, file_size, modified_ns) \
                     VALUES (?1, 'interrupted.cr3', 1, 1)",
                    [[3_u8; 32].as_slice()],
                )
                .expect("uncommitted insert");
            // Dropping without commit models process work interrupted before
            // the transaction becomes durable.
        }
        let count: u32 = catalog
            .connection
            .query_row("SELECT count(*) FROM assets", [], |row| row.get(0))
            .expect("asset count");
        assert_eq!(count, 0);
        assert!(catalog.integrity_check().is_ok());
        drop(catalog);
        let second_reader = Catalog::open(&database).expect("concurrent reader");
        let third_reader = Catalog::open(&database).expect("concurrent reader");
        second_reader.integrity_check().expect("integrity");
        third_reader.integrity_check().expect("integrity");
    }

    #[test]
    fn failed_migration_rolls_back_schema_and_version() {
        let mut connection = rusqlite::Connection::open_in_memory().expect("database");
        let result = apply_migration(
            &mut connection,
            1,
            "CREATE TABLE partial(id INTEGER); THIS IS NOT SQL;",
        );
        assert!(result.is_err());
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("version");
        let table_count: u32 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='partial'",
                [],
                |row| row.get(0),
            )
            .expect("schema");
        assert_eq!(version, 0);
        assert_eq!(table_count, 0);
    }

    #[test]
    fn clean_catalog_rebuild_reproduces_sidecar_edits_and_copies() {
        let directory = tempdir().expect("tempdir");
        let source = directory.path().join("photo.cr3");
        let sidecar = directory.path().join("photo.xmp");
        let database = directory.path().join("rebuilt.sqlite3");
        let original = b"immutable original bytes";
        fs::write(&source, original).expect("source");
        let mut base = EditRecipe::default();
        base.operator_mut(OperatorId::Exposure)
            .expect("exposure")
            .parameters = OperatorParameters::Exposure { ev: 0.75 };
        let payload = SidecarPayload {
            recipe: base.clone(),
            virtual_copies: vec![VirtualCopySidecar {
                name: "alternate".into(),
                recipe: EditRecipe::default(),
            }],
            ..SidecarPayload::default()
        };
        write_sidecar_atomic(&sidecar, &payload, None).expect("sidecar");
        let mut catalog = Catalog::open(&database).expect("catalog");
        let assets = catalog
            .rebuild_sources(&[RebuildSource {
                source_path: source.clone(),
                content_hash: [9; 32],
                file_size: original.len() as u64,
                modified_ns: 1,
                metadata_json: "{}".into(),
                sidecar_path: Some(sidecar),
            }])
            .expect("rebuild");
        assert_eq!(catalog.load_recipe(assets[0]).expect("recipe"), Some(base));
        assert_eq!(catalog.virtual_copies(assets[0]).expect("copies").len(), 1);
        assert_eq!(fs::read(source).expect("original"), original);
        catalog.integrity_check().expect("integrity");
    }
}
