//! Stable domain invariants shared by Greign subsystems.

use std::fmt;

mod history;
mod recipe;

pub use history::RecipeHistory;
pub use recipe::{EditRecipe, OperatorId, OperatorParameters, RecipeError, RecipeOperator};

/// Current serialized edit-recipe schema version.
pub const RECIPE_SCHEMA_VERSION: u32 = 1;

/// Current `SQLite` catalog schema version.
pub const CATALOG_SCHEMA_VERSION: u32 = 1;

/// Current ordered reference-pipeline behavior version.
pub const PIPELINE_VERSION: u32 = 2;

/// Versions which affect persisted data or compatibility reporting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VersionInfo {
    /// Greign package version.
    pub package: &'static str,
    /// Serialized edit-recipe schema version.
    pub recipe_schema: u32,
    /// `SQLite` catalog schema version.
    pub catalog_schema: u32,
    /// Reference processing behavior version.
    pub pipeline: u32,
}

impl VersionInfo {
    /// Returns the versions compiled into this build.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            package: env!("CARGO_PKG_VERSION"),
            recipe_schema: RECIPE_SCHEMA_VERSION,
            catalog_schema: CATALOG_SCHEMA_VERSION,
            pipeline: PIPELINE_VERSION,
        }
    }
}

impl fmt::Display for VersionInfo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "greign={} recipe_schema={} catalog_schema={} pipeline={}",
            self.package, self.recipe_schema, self.catalog_schema, self.pipeline
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{CATALOG_SCHEMA_VERSION, PIPELINE_VERSION, RECIPE_SCHEMA_VERSION, VersionInfo};

    #[test]
    fn current_version_info_uses_persistence_constants() {
        let info = VersionInfo::current();
        assert_eq!(info.recipe_schema, RECIPE_SCHEMA_VERSION);
        assert_eq!(info.catalog_schema, CATALOG_SCHEMA_VERSION);
        assert_eq!(info.pipeline, PIPELINE_VERSION);
        assert!(!info.package.is_empty());
    }

    #[test]
    fn version_info_has_stable_machine_readable_display() {
        assert_eq!(
            VersionInfo::current().to_string(),
            "greign=0.1.0 recipe_schema=1 catalog_schema=1 pipeline=2"
        );
    }
}
