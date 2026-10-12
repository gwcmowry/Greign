//! Recoverable device lifecycle state machine.

/// Monotonically increasing successful device initialization.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DeviceGeneration(pub u64);

/// GPU runtime state exposed to orchestration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceState {
    /// No adapter has been requested yet.
    Uninitialized,
    /// Device and queues are usable.
    Ready(DeviceGeneration),
    /// Device was lost and recovery may be attempted.
    Lost {
        /// Most recently lost generation.
        generation: DeviceGeneration,
        /// Consecutive failed recovery attempts.
        failed_attempts: u8,
    },
    /// Recovery limit was exceeded; CPU remains available.
    CpuOnly,
}

/// Action the owner should take after a state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    /// Initialize or recreate a device.
    RequestDevice,
    /// Continue rendering on the ready GPU.
    UseGpu,
    /// Route work through the CPU reference path.
    UseCpu,
}

/// Deterministic device-loss and recovery policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeLifecycle {
    state: DeviceState,
    max_recovery_attempts: u8,
    next_generation: u64,
}

impl RuntimeLifecycle {
    /// Creates an uninitialized lifecycle.
    #[must_use]
    pub const fn new(max_recovery_attempts: u8) -> Self {
        Self {
            state: DeviceState::Uninitialized,
            max_recovery_attempts,
            next_generation: 1,
        }
    }

    /// Current device state.
    #[must_use]
    pub const fn state(self) -> DeviceState {
        self.state
    }

    /// Action appropriate for the current state.
    #[must_use]
    pub const fn action(self) -> RecoveryAction {
        match self.state {
            DeviceState::Uninitialized | DeviceState::Lost { .. } => RecoveryAction::RequestDevice,
            DeviceState::Ready(_) => RecoveryAction::UseGpu,
            DeviceState::CpuOnly => RecoveryAction::UseCpu,
        }
    }

    /// Records successful initialization or recovery.
    pub fn device_ready(&mut self) {
        let generation = DeviceGeneration(self.next_generation);
        self.next_generation = self.next_generation.saturating_add(1);
        self.state = DeviceState::Ready(generation);
    }

    /// Records loss of the current device.
    pub fn device_lost(&mut self) {
        let generation = match self.state {
            DeviceState::Ready(generation) | DeviceState::Lost { generation, .. } => generation,
            DeviceState::Uninitialized | DeviceState::CpuOnly => DeviceGeneration(0),
        };
        self.state = DeviceState::Lost {
            generation,
            failed_attempts: 0,
        };
    }

    /// Records one failed recovery request and switches permanently to CPU
    /// after the configured attempt limit.
    pub fn recovery_failed(&mut self) {
        let DeviceState::Lost {
            generation,
            failed_attempts,
        } = self.state
        else {
            return;
        };
        let failed_attempts = failed_attempts.saturating_add(1);
        if failed_attempts >= self.max_recovery_attempts {
            self.state = DeviceState::CpuOnly;
        } else {
            self.state = DeviceState::Lost {
                generation,
                failed_attempts,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceGeneration, DeviceState, RecoveryAction, RuntimeLifecycle};

    #[test]
    fn device_loss_recovers_into_new_generation() {
        let mut lifecycle = RuntimeLifecycle::new(3);
        assert_eq!(lifecycle.action(), RecoveryAction::RequestDevice);
        lifecycle.device_ready();
        assert_eq!(lifecycle.state(), DeviceState::Ready(DeviceGeneration(1)));
        lifecycle.device_lost();
        assert_eq!(lifecycle.action(), RecoveryAction::RequestDevice);
        lifecycle.device_ready();
        assert_eq!(lifecycle.state(), DeviceState::Ready(DeviceGeneration(2)));
    }

    #[test]
    fn repeated_recovery_failure_falls_back_to_cpu() {
        let mut lifecycle = RuntimeLifecycle::new(2);
        lifecycle.device_ready();
        lifecycle.device_lost();
        lifecycle.recovery_failed();
        assert!(matches!(lifecycle.state(), DeviceState::Lost { .. }));
        lifecycle.recovery_failed();
        assert_eq!(lifecycle.state(), DeviceState::CpuOnly);
        assert_eq!(lifecycle.action(), RecoveryAction::UseCpu);
    }
}
