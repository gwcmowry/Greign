//! Explicit GPU allocation accounting.

use std::collections::HashMap;
use std::num::NonZeroU64;

use std::fmt;

/// Opaque allocation handle owned by the future texture/buffer pool.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AllocationId(NonZeroU64);

/// Current and peak residency.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryReport {
    /// Configured hard limit.
    pub limit_bytes: u64,
    /// Currently reserved bytes.
    pub used_bytes: u64,
    /// Highest observed reservation.
    pub peak_bytes: u64,
    /// Live allocation count.
    pub allocations: usize,
}

/// Allocation-accounting failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    /// Requested reservation would cross the configured budget.
    BudgetExceeded {
        /// Requested bytes.
        requested: u64,
        /// Currently resident bytes.
        used: u64,
        /// Configured limit.
        limit: u64,
    },
    /// Arithmetic overflow while calculating residency.
    Overflow,
    /// Released handle is not live.
    UnknownAllocation,
}

impl fmt::Display for MemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetExceeded {
                requested,
                used,
                limit,
            } => write!(
                formatter,
                "GPU allocation of {requested} bytes exceeds budget: {used}/{limit} bytes used"
            ),
            Self::Overflow => formatter.write_str("GPU memory accounting overflow"),
            Self::UnknownAllocation => formatter.write_str("unknown GPU allocation handle"),
        }
    }
}

impl std::error::Error for MemoryError {}

/// Hard memory budget shared by GPU texture and buffer pools.
#[derive(Debug)]
pub struct MemoryBudget {
    limit_bytes: u64,
    used_bytes: u64,
    peak_bytes: u64,
    next_id: u64,
    allocations: HashMap<AllocationId, u64>,
}

impl MemoryBudget {
    /// Creates an empty budget.
    #[must_use]
    pub fn new(limit_bytes: u64) -> Self {
        Self {
            limit_bytes,
            used_bytes: 0,
            peak_bytes: 0,
            next_id: 1,
            allocations: HashMap::new(),
        }
    }

    /// Reserves bytes before creating a GPU resource.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError`] on overflow or when the budget would be crossed.
    pub fn reserve(&mut self, bytes: u64) -> Result<AllocationId, MemoryError> {
        let new_used = self
            .used_bytes
            .checked_add(bytes)
            .ok_or(MemoryError::Overflow)?;
        if new_used > self.limit_bytes {
            return Err(MemoryError::BudgetExceeded {
                requested: bytes,
                used: self.used_bytes,
                limit: self.limit_bytes,
            });
        }
        let raw_id = NonZeroU64::new(self.next_id).ok_or(MemoryError::Overflow)?;
        self.next_id = self.next_id.checked_add(1).ok_or(MemoryError::Overflow)?;
        let id = AllocationId(raw_id);
        self.allocations.insert(id, bytes);
        self.used_bytes = new_used;
        self.peak_bytes = self.peak_bytes.max(new_used);
        Ok(id)
    }

    /// Releases a live resource reservation.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::UnknownAllocation`] for an invalid or duplicate release.
    pub fn release(&mut self, id: AllocationId) -> Result<(), MemoryError> {
        let bytes = self
            .allocations
            .remove(&id)
            .ok_or(MemoryError::UnknownAllocation)?;
        self.used_bytes -= bytes;
        Ok(())
    }

    /// Returns current accounting statistics.
    #[must_use]
    pub fn report(&self) -> MemoryReport {
        MemoryReport {
            limit_bytes: self.limit_bytes,
            used_bytes: self.used_bytes,
            peak_bytes: self.peak_bytes,
            allocations: self.allocations.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryBudget, MemoryError};

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn two_gib_budget_never_overcommits() {
        let mut budget = MemoryBudget::new(2 * GIB);
        let first = budget.reserve(GIB).expect("first allocation");
        let second = budget.reserve(GIB).expect("second allocation");
        assert!(matches!(
            budget.reserve(1),
            Err(MemoryError::BudgetExceeded { .. })
        ));
        budget.release(first).expect("release first");
        budget.release(second).expect("release second");
        assert_eq!(budget.report().used_bytes, 0);
        assert_eq!(budget.report().peak_bytes, 2 * GIB);
    }

    #[test]
    fn twelve_gib_budget_tracks_peak_not_cumulative_churn() {
        let mut budget = MemoryBudget::new(12 * GIB);
        let allocation = budget.reserve(8 * GIB).expect("allocation");
        budget.release(allocation).expect("release");
        let next = budget.reserve(9 * GIB).expect("next allocation");
        budget.release(next).expect("release next");
        assert_eq!(budget.report().peak_bytes, 9 * GIB);
    }

    #[test]
    fn duplicate_release_is_detected() {
        let mut budget = MemoryBudget::new(1_024);
        let allocation = budget.reserve(1).expect("allocation");
        budget.release(allocation).expect("first release");
        assert_eq!(
            budget.release(allocation),
            Err(MemoryError::UnknownAllocation)
        );
    }
}
