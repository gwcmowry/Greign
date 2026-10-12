//! Undo/redo command history with continuous-edit coalescing.

use crate::EditRecipe;

/// Bounded recipe history for one virtual copy.
#[derive(Clone, Debug)]
pub struct RecipeHistory {
    current: EditRecipe,
    undo: Vec<EditRecipe>,
    redo: Vec<EditRecipe>,
    coalescing_key: Option<String>,
    capacity: usize,
}

impl RecipeHistory {
    /// Creates a history with a positive undo capacity.
    #[must_use]
    pub fn new(initial: EditRecipe, capacity: usize) -> Option<Self> {
        (capacity > 0).then(|| Self {
            current: initial,
            undo: Vec::new(),
            redo: Vec::new(),
            coalescing_key: None,
            capacity,
        })
    }

    /// Current recipe.
    #[must_use]
    pub const fn current(&self) -> &EditRecipe {
        &self.current
    }

    /// Applies a complete next recipe.
    ///
    /// Repeated calls with the same non-empty coalescing key create one undo
    /// step, matching a continuous slider gesture.
    pub fn apply(&mut self, next: EditRecipe, coalescing_key: Option<&str>) {
        let continues =
            coalescing_key.is_some() && self.coalescing_key.as_deref() == coalescing_key;
        if !continues {
            self.undo.push(self.current.clone());
            if self.undo.len() > self.capacity {
                self.undo.remove(0);
            }
        }
        self.current = next;
        self.redo.clear();
        self.coalescing_key = coalescing_key.map(str::to_owned);
    }

    /// Ends continuous-edit coalescing without changing the recipe.
    pub fn commit_gesture(&mut self) {
        self.coalescing_key = None;
    }

    /// Restores the previous recipe, if any.
    pub fn undo(&mut self) -> bool {
        self.coalescing_key = None;
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.current, previous));
        true
    }

    /// Reapplies the last undone recipe, if any.
    pub fn redo(&mut self) -> bool {
        self.coalescing_key = None;
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.current, next));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::RecipeHistory;
    use crate::{EditRecipe, OperatorId, OperatorParameters};

    fn exposure(mut recipe: EditRecipe, ev: f32) -> EditRecipe {
        let operator = recipe.operator_mut(OperatorId::Exposure).expect("exposure");
        operator.parameters = OperatorParameters::Exposure { ev };
        recipe
    }

    #[test]
    fn slider_updates_coalesce_into_one_undo_step() {
        let initial = EditRecipe::default();
        let mut history = RecipeHistory::new(initial.clone(), 10).expect("history");
        history.apply(exposure(initial.clone(), 0.5), Some("exposure"));
        history.apply(exposure(initial.clone(), 1.0), Some("exposure"));
        history.apply(exposure(initial.clone(), 1.5), Some("exposure"));
        assert!(history.undo());
        assert_eq!(history.current(), &initial);
        assert!(!history.undo());
        assert!(history.redo());
        assert_eq!(
            history
                .current()
                .operator(OperatorId::Exposure)
                .expect("exposure")
                .parameters,
            OperatorParameters::Exposure { ev: 1.5 }
        );
    }

    #[test]
    fn new_gesture_clears_redo_branch() {
        let initial = EditRecipe::default();
        let mut history = RecipeHistory::new(initial.clone(), 10).expect("history");
        history.apply(exposure(initial.clone(), 1.0), None);
        assert!(history.undo());
        history.apply(exposure(initial, -1.0), None);
        assert!(!history.redo());
    }
}
