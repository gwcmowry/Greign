//! Versioned non-destructive edit recipe.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{PIPELINE_VERSION, RECIPE_SCHEMA_VERSION};

/// Ordered, versioned processing recipe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditRecipe {
    /// Serialized recipe schema.
    pub schema_version: u32,
    /// Rendering behavior expected by the recipe.
    pub pipeline_version: u32,
    /// Operators in processing order.
    pub operators: Vec<RecipeOperator>,
}

impl Default for EditRecipe {
    fn default() -> Self {
        Self {
            schema_version: RECIPE_SCHEMA_VERSION,
            pipeline_version: PIPELINE_VERSION,
            operators: vec![
                RecipeOperator::new(OperatorParameters::WhiteBalance {
                    coefficients: [1.0; 3],
                }),
                RecipeOperator::new(OperatorParameters::Highlight { threshold: 0.995 }),
                RecipeOperator::new(OperatorParameters::ChromaDenoise { strength: 0.5 }),
                RecipeOperator::new(OperatorParameters::Exposure { ev: 0.0 }),
                RecipeOperator::new(OperatorParameters::CaptureSharpen { amount: 0.2 }),
                RecipeOperator::new(OperatorParameters::Tone { strength: 1.0 }),
            ],
        }
    }
}

impl EditRecipe {
    /// Parses and validates a recipe JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] for malformed JSON, unsupported versions,
    /// duplicate operators, or non-finite/out-of-range parameters.
    pub fn from_json(json: &str) -> Result<Self, RecipeError> {
        let mut recipe: Self = serde_json::from_str(json)?;
        recipe.canonicalize()?;
        Ok(recipe)
    }

    /// Validates and emits deterministic compact JSON.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] for unsupported versions, duplicate operators,
    /// or non-finite/out-of-range parameters.
    pub fn to_canonical_json(&self) -> Result<String, RecipeError> {
        let mut canonical = self.clone();
        canonical.canonicalize()?;
        Ok(serde_json::to_string(&canonical)?)
    }

    /// Returns one operator by stable identifier.
    #[must_use]
    pub fn operator(&self, id: OperatorId) -> Option<&RecipeOperator> {
        self.operators.iter().find(|operator| operator.id() == id)
    }

    /// Returns one mutable operator by stable identifier.
    pub fn operator_mut(&mut self, id: OperatorId) -> Option<&mut RecipeOperator> {
        self.operators
            .iter_mut()
            .find(|operator| operator.id() == id)
    }

    fn canonicalize(&mut self) -> Result<(), RecipeError> {
        if self.schema_version != RECIPE_SCHEMA_VERSION {
            return Err(RecipeError::UnsupportedSchema(self.schema_version));
        }
        if self.pipeline_version != PIPELINE_VERSION {
            return Err(RecipeError::UnsupportedPipeline(self.pipeline_version));
        }
        let mut ids = HashSet::new();
        for operator in &mut self.operators {
            if !ids.insert(operator.id()) {
                return Err(RecipeError::DuplicateOperator(operator.id()));
            }
            operator.canonicalize()?;
        }
        Ok(())
    }
}

/// One enabled/disabled versioned operator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecipeOperator {
    /// Operator implementation schema.
    pub version: u32,
    /// Whether processing applies this operation.
    pub enabled: bool,
    /// Typed parameters and stable operator identifier.
    #[serde(flatten)]
    pub parameters: OperatorParameters,
}

impl RecipeOperator {
    /// Creates an enabled version-one operator.
    #[must_use]
    pub const fn new(parameters: OperatorParameters) -> Self {
        Self {
            version: 1,
            enabled: true,
            parameters,
        }
    }

    /// Stable identifier derived from the parameter schema.
    #[must_use]
    pub const fn id(&self) -> OperatorId {
        self.parameters.id()
    }

    fn canonicalize(&mut self) -> Result<(), RecipeError> {
        if self.version != 1 {
            return Err(RecipeError::UnsupportedOperatorVersion {
                id: self.id(),
                version: self.version,
            });
        }
        self.parameters.canonicalize()
    }
}

/// Stable operator identifiers.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorId {
    /// Sensor-domain white balance.
    WhiteBalance,
    /// Highlight reconstruction threshold.
    Highlight,
    /// Scene-linear exposure.
    Exposure,
    /// Scene-to-display tone strength.
    Tone,
    /// Initial detail sharpening.
    CaptureSharpen,
    /// Edge-aware chroma-noise reduction.
    ChromaDenoise,
    /// Non-destructive crop rectangle.
    Crop,
}

/// Typed operator parameters using an explicit tagged representation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operator", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperatorParameters {
    /// RGB sensor coefficients.
    WhiteBalance {
        /// Positive coefficients in RGB order.
        coefficients: [f32; 3],
    },
    /// Sensor highlight reconstruction.
    Highlight {
        /// Positive normalized threshold.
        threshold: f32,
    },
    /// Exposure compensation.
    Exposure {
        /// Stops of compensation.
        ev: f32,
    },
    /// Tone mapping.
    Tone {
        /// Non-negative curve strength.
        strength: f32,
    },
    /// Capture sharpening.
    CaptureSharpen {
        /// Conservative amount from zero through two.
        amount: f32,
    },
    /// Chroma-noise reduction before exposure and sharpening.
    ChromaDenoise {
        /// Strength from zero (off) through one.
        strength: f32,
    },
    /// Crop in normalized source coordinates.
    Crop {
        /// Left coordinate in `[0, 1]`.
        x: f32,
        /// Top coordinate in `[0, 1]`.
        y: f32,
        /// Positive normalized width.
        width: f32,
        /// Positive normalized height.
        height: f32,
    },
}

impl OperatorParameters {
    /// Stable identifier for this parameter variant.
    #[must_use]
    pub const fn id(&self) -> OperatorId {
        match self {
            Self::WhiteBalance { .. } => OperatorId::WhiteBalance,
            Self::Highlight { .. } => OperatorId::Highlight,
            Self::Exposure { .. } => OperatorId::Exposure,
            Self::Tone { .. } => OperatorId::Tone,
            Self::CaptureSharpen { .. } => OperatorId::CaptureSharpen,
            Self::ChromaDenoise { .. } => OperatorId::ChromaDenoise,
            Self::Crop { .. } => OperatorId::Crop,
        }
    }

    fn canonicalize(&mut self) -> Result<(), RecipeError> {
        match self {
            Self::WhiteBalance { coefficients } => {
                for coefficient in coefficients {
                    normalize_zero(coefficient)?;
                    if *coefficient <= 0.0 {
                        return Err(RecipeError::InvalidParameter(self.id()));
                    }
                }
            }
            Self::Highlight { threshold } => {
                normalize_zero(threshold)?;
                if *threshold <= 0.0 {
                    return Err(RecipeError::InvalidParameter(self.id()));
                }
            }
            Self::ChromaDenoise { strength } => {
                normalize_zero(strength)?;
                if !(0.0..=1.0).contains(strength) {
                    return Err(RecipeError::InvalidParameter(self.id()));
                }
            }
            Self::Exposure { ev } => normalize_zero(ev)?,
            Self::Tone { strength } => {
                normalize_zero(strength)?;
                if *strength < 0.0 {
                    return Err(RecipeError::InvalidParameter(self.id()));
                }
            }
            Self::CaptureSharpen { amount } => {
                normalize_zero(amount)?;
                if !(0.0..=2.0).contains(amount) {
                    return Err(RecipeError::InvalidParameter(self.id()));
                }
            }
            Self::Crop {
                x,
                y,
                width,
                height,
            } => {
                for value in [&mut *x, &mut *y, &mut *width, &mut *height] {
                    normalize_zero(value)?;
                }
                let valid = *x >= 0.0
                    && *y >= 0.0
                    && *width > 0.0
                    && *height > 0.0
                    && *x + *width <= 1.0
                    && *y + *height <= 1.0;
                if !valid {
                    return Err(RecipeError::InvalidParameter(self.id()));
                }
            }
        }
        Ok(())
    }
}

fn normalize_zero(value: &mut f32) -> Result<(), RecipeError> {
    if !value.is_finite() {
        return Err(RecipeError::NonFiniteParameter);
    }
    if *value == 0.0 {
        *value = 0.0;
    }
    Ok(())
}

/// Recipe parse or invariant failure.
#[derive(Debug, Error)]
pub enum RecipeError {
    /// JSON syntax or representation failure.
    #[error("invalid recipe JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// Recipe schema is newer or older than this build.
    #[error("unsupported recipe schema {0}")]
    UnsupportedSchema(u32),
    /// Pipeline behavior differs from this build.
    #[error("unsupported pipeline version {0}")]
    UnsupportedPipeline(u32),
    /// Operator version is unsupported.
    #[error("unsupported {id:?} operator version {version}")]
    UnsupportedOperatorVersion {
        /// Stable operator identifier.
        id: OperatorId,
        /// Encountered version.
        version: u32,
    },
    /// Only one instance of each current global operator is accepted.
    #[error("duplicate recipe operator {0:?}")]
    DuplicateOperator(OperatorId),
    /// A parameter violates its operator domain.
    #[error("invalid parameter for recipe operator {0:?}")]
    InvalidParameter(OperatorId),
    /// JSON cannot represent NaN or infinity.
    #[error("recipe parameter must be finite")]
    NonFiniteParameter,
}

#[cfg(test)]
mod tests {
    use super::{EditRecipe, OperatorId, OperatorParameters, RecipeOperator};

    #[test]
    fn canonical_json_round_trip_is_lossless_and_stable() {
        let recipe = EditRecipe::default();
        let first = recipe.to_canonical_json().expect("serialize");
        let reparsed = EditRecipe::from_json(&first).expect("parse");
        let second = reparsed.to_canonical_json().expect("serialize");
        assert_eq!(recipe, reparsed);
        assert_eq!(first, second);
    }

    #[test]
    fn negative_zero_has_one_canonical_encoding() {
        let mut recipe = EditRecipe::default();
        recipe
            .operators
            .push(RecipeOperator::new(OperatorParameters::Crop {
                x: -0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            }));
        let json = recipe.to_canonical_json().expect("serialize");
        assert!(!json.contains("-0.0"));
    }

    #[test]
    fn duplicate_global_operator_is_rejected() {
        let mut recipe = EditRecipe::default();
        recipe
            .operators
            .push(RecipeOperator::new(OperatorParameters::Exposure {
                ev: 1.0,
            }));
        assert!(recipe.to_canonical_json().is_err());
    }

    #[test]
    fn typed_operator_lookup_is_stable() {
        assert!(
            EditRecipe::default()
                .operator(OperatorId::Exposure)
                .is_some()
        );
    }
}
