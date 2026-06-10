pub const FRONT_PANEL_DIM_AFTER_MS: u64 = 30_000;
pub const FRONT_PANEL_BACKLIGHT_OFF_AFTER_MS: u64 = 35_000;
pub const FRONT_PANEL_SLEEP_AFTER_MS: u64 = 40_000;

pub const FRONT_PANEL_RELEASE_DIM_AFTER_MS: u64 = 180_000;
pub const FRONT_PANEL_RELEASE_BACKLIGHT_OFF_AFTER_MS: u64 = 240_000;
pub const FRONT_PANEL_RELEASE_SLEEP_AFTER_MS: u64 = 245_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayPowerMode {
    Awake,
    Dimmed,
    BacklightOff,
    Sleeping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayPowerCommand {
    None,
    FullBrightness,
    Dim,
    BacklightOff,
    Sleep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayPowerPolicy {
    pub dim_after_ms: u64,
    pub backlight_off_after_ms: u64,
    pub sleep_after_ms: u64,
}

impl DisplayPowerPolicy {
    pub const fn test_default() -> Self {
        Self {
            dim_after_ms: FRONT_PANEL_DIM_AFTER_MS,
            backlight_off_after_ms: FRONT_PANEL_BACKLIGHT_OFF_AFTER_MS,
            sleep_after_ms: FRONT_PANEL_SLEEP_AFTER_MS,
        }
    }

    pub const fn release_default() -> Self {
        Self {
            dim_after_ms: FRONT_PANEL_RELEASE_DIM_AFTER_MS,
            backlight_off_after_ms: FRONT_PANEL_RELEASE_BACKLIGHT_OFF_AFTER_MS,
            sleep_after_ms: FRONT_PANEL_RELEASE_SLEEP_AFTER_MS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayPowerController {
    policy: DisplayPowerPolicy,
    idle_started_ms: u64,
    mode: DisplayPowerMode,
}

impl DisplayPowerController {
    pub const fn new(policy: DisplayPowerPolicy, now_ms: u64) -> Self {
        Self {
            policy,
            idle_started_ms: now_ms,
            mode: DisplayPowerMode::Awake,
        }
    }

    pub const fn mode(&self) -> DisplayPowerMode {
        self.mode
    }

    pub fn reset(&mut self, now_ms: u64) -> DisplayPowerCommand {
        self.idle_started_ms = now_ms;
        self.transition_to(DisplayPowerMode::Awake)
    }

    pub fn step(
        &mut self,
        now_ms: u64,
        user_activity: bool,
        attention_hold: bool,
    ) -> DisplayPowerCommand {
        if user_activity || attention_hold {
            return self.reset(now_ms);
        }

        let idle_ms = now_ms.saturating_sub(self.idle_started_ms);
        let target = if idle_ms >= self.policy.sleep_after_ms {
            DisplayPowerMode::Sleeping
        } else if idle_ms >= self.policy.backlight_off_after_ms {
            DisplayPowerMode::BacklightOff
        } else if idle_ms >= self.policy.dim_after_ms {
            DisplayPowerMode::Dimmed
        } else {
            DisplayPowerMode::Awake
        };

        self.transition_to(target)
    }

    fn transition_to(&mut self, target: DisplayPowerMode) -> DisplayPowerCommand {
        if self.mode == target {
            return DisplayPowerCommand::None;
        }

        self.mode = target;
        match target {
            DisplayPowerMode::Awake => DisplayPowerCommand::FullBrightness,
            DisplayPowerMode::Dimmed => DisplayPowerCommand::Dim,
            DisplayPowerMode::BacklightOff => DisplayPowerCommand::BacklightOff,
            DisplayPowerMode::Sleeping => DisplayPowerCommand::Sleep,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller() -> DisplayPowerController {
        DisplayPowerController::new(DisplayPowerPolicy::test_default(), 0)
    }

    #[test]
    fn release_default_uses_longer_idle_timing() {
        let policy = DisplayPowerPolicy::release_default();

        assert_eq!(policy.dim_after_ms, FRONT_PANEL_RELEASE_DIM_AFTER_MS);
        assert_eq!(
            policy.backlight_off_after_ms,
            FRONT_PANEL_RELEASE_BACKLIGHT_OFF_AFTER_MS
        );
        assert_eq!(policy.sleep_after_ms, FRONT_PANEL_RELEASE_SLEEP_AFTER_MS);
    }

    #[test]
    fn compressed_test_timing_transitions_through_dim_off_sleep() {
        let mut power = controller();

        assert_eq!(power.step(29_999, false, false), DisplayPowerCommand::None);
        assert_eq!(power.mode(), DisplayPowerMode::Awake);

        assert_eq!(power.step(30_000, false, false), DisplayPowerCommand::Dim);
        assert_eq!(power.mode(), DisplayPowerMode::Dimmed);

        assert_eq!(
            power.step(35_000, false, false),
            DisplayPowerCommand::BacklightOff
        );
        assert_eq!(power.mode(), DisplayPowerMode::BacklightOff);

        assert_eq!(power.step(40_000, false, false), DisplayPowerCommand::Sleep);
        assert_eq!(power.mode(), DisplayPowerMode::Sleeping);
    }

    #[test]
    fn activity_wakes_and_restarts_idle_timer() {
        let mut power = controller();
        assert_eq!(power.step(40_000, false, false), DisplayPowerCommand::Sleep);

        assert_eq!(
            power.step(41_000, true, false),
            DisplayPowerCommand::FullBrightness
        );
        assert_eq!(power.mode(), DisplayPowerMode::Awake);

        assert_eq!(power.step(70_999, false, false), DisplayPowerCommand::None);
        assert_eq!(power.mode(), DisplayPowerMode::Awake);
        assert_eq!(power.step(71_000, false, false), DisplayPowerCommand::Dim);
    }

    #[test]
    fn attention_hold_keeps_awake_and_restarts_timer_after_release() {
        let mut power = controller();

        assert_eq!(power.step(30_000, false, true), DisplayPowerCommand::None);
        assert_eq!(power.mode(), DisplayPowerMode::Awake);

        assert_eq!(power.step(59_999, false, false), DisplayPowerCommand::None);
        assert_eq!(power.mode(), DisplayPowerMode::Awake);
        assert_eq!(power.step(60_000, false, false), DisplayPowerCommand::Dim);
    }

    #[test]
    fn attention_hold_wakes_from_sleep() {
        let mut power = controller();
        assert_eq!(power.step(40_000, false, false), DisplayPowerCommand::Sleep);

        assert_eq!(
            power.step(41_000, false, true),
            DisplayPowerCommand::FullBrightness
        );
        assert_eq!(power.mode(), DisplayPowerMode::Awake);
    }
}
