#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanLevel {
    Off,
    Low,
    Mid,
    High,
}

impl FanLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Mid => "mid",
            Self::High => "high",
        }
    }

    pub const fn enabled(self) -> bool {
        !matches!(self, Self::Off)
    }

    pub const fn from_pwm_pct(pwm_pct: u8) -> Self {
        match pwm_pct {
            0 => Self::Off,
            1..=33 => Self::Low,
            34..=66 => Self::Mid,
            _ => Self::High,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempSource {
    Pending,
    Missing,
    TmpA,
    TmpB,
    Bms,
    Max,
}

impl TempSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Missing => "missing",
            Self::TmpA => "tmp_a",
            Self::TmpB => "tmp_b",
            Self::Bms => "bms",
            Self::Max => "max",
        }
    }

    pub const fn has_control_temp(self) -> bool {
        matches!(self, Self::TmpA | Self::TmpB | Self::Bms | Self::Max)
    }

    pub const fn is_degraded(self) -> bool {
        matches!(self, Self::Missing | Self::TmpA | Self::TmpB | Self::Bms)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub stop_temp_c_x16: i16,
    pub target_temp_c_x16: i16,
    pub min_run_pwm_pct: u8,
    pub step_down_pwm_pct: u8,
    pub step_up_small_delta_c_x16: i16,
    pub step_up_medium_delta_c_x16: i16,
    pub step_up_small_pwm_pct: u8,
    pub step_up_medium_pwm_pct: u8,
    pub step_up_large_pwm_pct: u8,
    pub control_interval_ms: u64,
    pub tach_timeout_ms: u64,
    pub tach_pulses_per_rev: u8,
    pub tach_watchdog_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub now_ms: u64,
    pub temps_ready: bool,
    pub temp_a_c_x16: Option<i16>,
    pub temp_b_c_x16: Option<i16>,
    pub temp_bms_c_x16: Option<i16>,
    pub tach_pulse_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub requested_command: FanLevel,
    pub requested_pwm_pct: u8,
    pub command: FanLevel,
    pub pwm_pct: u8,
    pub temp_source: TempSource,
    pub control_temp_c_x16: Option<i16>,
    pub tach_fault: bool,
    pub tach_pulse_seen_recently: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Events {
    pub output_changed: bool,
    pub temp_source_changed: bool,
    pub tach_fault_changed: bool,
}

pub struct Controller {
    cfg: Config,
    requested_pwm_pct: u8,
    last_control_at_ms: Option<u64>,
    last_tach_seen_ms: Option<u64>,
    tach_recovery_started_ms: Option<u64>,
    tach_recovery_pulses: u32,
    status: Status,
}

impl Controller {
    const TACH_RECOVERY_CONFIRM_MS: u64 = 20;

    pub const fn new(cfg: Config) -> Self {
        Self {
            cfg,
            requested_pwm_pct: 0,
            last_control_at_ms: None,
            last_tach_seen_ms: None,
            tach_recovery_started_ms: None,
            tach_recovery_pulses: 0,
            status: Status {
                requested_command: FanLevel::Off,
                requested_pwm_pct: 0,
                command: FanLevel::Off,
                pwm_pct: 0,
                temp_source: TempSource::Pending,
                control_temp_c_x16: None,
                tach_fault: false,
                tach_pulse_seen_recently: false,
            },
        }
    }

    pub const fn config(&self) -> Config {
        self.cfg
    }

    pub const fn status(&self) -> Status {
        self.status
    }

    pub fn update(&mut self, input: Input) -> (Status, Events) {
        let prev = self.status;
        let (control_temp_c_x16, temp_source) = select_control_temp(
            input.temps_ready,
            input.temp_a_c_x16,
            input.temp_b_c_x16,
            input.temp_bms_c_x16,
        );

        let requested_pwm_pct = match temp_source {
            TempSource::Pending => 0,
            TempSource::Missing => 100,
            _ => self.next_requested_pwm_pct(
                input.now_ms,
                control_temp_c_x16.expect("control temp must exist when source is present"),
            ),
        };
        self.requested_pwm_pct = requested_pwm_pct;
        let requested_command = FanLevel::from_pwm_pct(requested_pwm_pct);

        let expecting_tach =
            self.cfg.tach_watchdog_enabled && (prev.tach_fault || requested_pwm_pct > 0);
        let mut tach_fault = prev.tach_fault;
        if input.tach_pulse_count > 0 {
            self.last_tach_seen_ms = Some(input.now_ms);
            if prev.tach_fault {
                let started_ms = self.tach_recovery_started_ms.get_or_insert(input.now_ms);
                self.tach_recovery_pulses = self
                    .tach_recovery_pulses
                    .saturating_add(input.tach_pulse_count);
                if self.tach_recovery_pulses >= 2
                    && input.now_ms.saturating_sub(*started_ms) >= Self::TACH_RECOVERY_CONFIRM_MS
                {
                    tach_fault = false;
                    self.tach_recovery_started_ms = None;
                    self.tach_recovery_pulses = 0;
                }
            } else {
                self.tach_recovery_started_ms = None;
                self.tach_recovery_pulses = 0;
                tach_fault = false;
            }
        } else if expecting_tach {
            if self
                .last_tach_seen_ms
                .map(|last_seen_ms| {
                    input.now_ms.saturating_sub(last_seen_ms) >= self.cfg.tach_timeout_ms
                })
                .unwrap_or(true)
            {
                self.tach_recovery_started_ms = None;
                self.tach_recovery_pulses = 0;
            }
            if !prev.command.enabled() && !prev.tach_fault {
                self.last_tach_seen_ms = Some(input.now_ms);
            }
            if let Some(last_seen_ms) = self.last_tach_seen_ms {
                if input.now_ms.saturating_sub(last_seen_ms) >= self.cfg.tach_timeout_ms {
                    tach_fault = true;
                }
            }
        } else {
            tach_fault = false;
            self.tach_recovery_started_ms = None;
            self.tach_recovery_pulses = 0;
        }

        let (command, pwm_pct) = if tach_fault {
            (FanLevel::High, 100)
        } else {
            (requested_command, requested_pwm_pct)
        };
        let tach_pulse_seen_recently = if expecting_tach {
            self.last_tach_seen_ms
                .map(|last_seen_ms| {
                    input.now_ms.saturating_sub(last_seen_ms) < self.cfg.tach_timeout_ms
                })
                .unwrap_or(false)
        } else {
            false
        };

        self.status = Status {
            requested_command,
            requested_pwm_pct,
            command,
            pwm_pct,
            temp_source,
            control_temp_c_x16,
            tach_fault,
            tach_pulse_seen_recently,
        };

        (
            self.status,
            Events {
                output_changed: self.status.command != prev.command
                    || self.status.pwm_pct != prev.pwm_pct
                    || self.status.requested_command != prev.requested_command
                    || self.status.requested_pwm_pct != prev.requested_pwm_pct,
                temp_source_changed: self.status.temp_source != prev.temp_source,
                tach_fault_changed: self.status.tach_fault != prev.tach_fault,
            },
        )
    }

    fn next_requested_pwm_pct(&mut self, now_ms: u64, control_temp_c_x16: i16) -> u8 {
        let should_step = self
            .last_control_at_ms
            .map(|last_ms| now_ms.saturating_sub(last_ms) >= self.cfg.control_interval_ms)
            .unwrap_or(true);

        if !should_step {
            return self.requested_pwm_pct;
        }
        self.last_control_at_ms = Some(now_ms);

        adjust_pwm_pct(self.requested_pwm_pct, control_temp_c_x16, self.cfg)
    }
}

fn adjust_pwm_pct(current_pwm_pct: u8, temp_c_x16: i16, cfg: Config) -> u8 {
    if temp_c_x16 < cfg.stop_temp_c_x16 {
        return 0;
    }

    if temp_c_x16 < cfg.target_temp_c_x16 {
        if current_pwm_pct == 0 {
            return cfg.min_run_pwm_pct;
        }
        return current_pwm_pct
            .saturating_sub(cfg.step_down_pwm_pct)
            .max(cfg.min_run_pwm_pct);
    }

    let step_up = if temp_c_x16 >= cfg.target_temp_c_x16 + cfg.step_up_medium_delta_c_x16 {
        cfg.step_up_large_pwm_pct
    } else if temp_c_x16 >= cfg.target_temp_c_x16 + cfg.step_up_small_delta_c_x16 {
        cfg.step_up_medium_pwm_pct
    } else {
        cfg.step_up_small_pwm_pct
    };

    current_pwm_pct
        .max(cfg.min_run_pwm_pct)
        .saturating_add(step_up)
        .clamp(cfg.min_run_pwm_pct, 100)
}

fn select_control_temp(
    temps_ready: bool,
    temp_a_c_x16: Option<i16>,
    temp_b_c_x16: Option<i16>,
    temp_bms_c_x16: Option<i16>,
) -> (Option<i16>, TempSource) {
    if !temps_ready {
        return (None, TempSource::Pending);
    }

    let mut control_temp_c_x16 = None;
    let mut seen_count = 0u8;
    let mut last_source = TempSource::Missing;

    for (source, sample) in [
        (TempSource::TmpA, temp_a_c_x16),
        (TempSource::TmpB, temp_b_c_x16),
        (TempSource::Bms, temp_bms_c_x16),
    ] {
        if let Some(sample_c_x16) = sample {
            control_temp_c_x16 =
                Some(control_temp_c_x16.map_or(sample_c_x16, |cur: i16| cur.max(sample_c_x16)));
            seen_count = seen_count.saturating_add(1);
            last_source = source;
        }
    }

    match seen_count {
        0 => (None, TempSource::Missing),
        1 => (control_temp_c_x16, last_source),
        _ => (control_temp_c_x16, TempSource::Max),
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, Controller, FanLevel, Input, TempSource};

    fn cfg() -> Config {
        Config {
            stop_temp_c_x16: 37 * 16,
            target_temp_c_x16: 40 * 16,
            min_run_pwm_pct: 10,
            step_down_pwm_pct: 5,
            step_up_small_delta_c_x16: 1 * 16,
            step_up_medium_delta_c_x16: 3 * 16,
            step_up_small_pwm_pct: 5,
            step_up_medium_pwm_pct: 10,
            step_up_large_pwm_pct: 15,
            control_interval_ms: 500,
            tach_timeout_ms: 2_000,
            tach_pulses_per_rev: 2,
            tach_watchdog_enabled: true,
        }
    }

    #[test]
    fn ramps_up_toward_target_in_steps() {
        let mut ctl = Controller::new(cfg());

        let (status, _) = ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(40 * 16),
            temp_b_c_x16: Some(39 * 16),
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.requested_pwm_pct, 15);
        assert_eq!(status.command, FanLevel::Low);

        let (status, _) = ctl.update(Input {
            now_ms: 500,
            temps_ready: true,
            temp_a_c_x16: Some(42 * 16),
            temp_b_c_x16: Some(39 * 16),
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.requested_pwm_pct, 25);
        assert_eq!(status.command, FanLevel::Low);

        let (status, _) = ctl.update(Input {
            now_ms: 1_000,
            temps_ready: true,
            temp_a_c_x16: Some(44 * 16),
            temp_b_c_x16: Some(39 * 16),
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.requested_pwm_pct, 40);
        assert_eq!(status.command, FanLevel::Mid);
    }

    #[test]
    fn drops_to_minimum_then_stops_below_stop_threshold() {
        let mut ctl = Controller::new(cfg());

        ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(43 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });

        let (status, _) = ctl.update(Input {
            now_ms: 500,
            temps_ready: true,
            temp_a_c_x16: Some(39 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.requested_pwm_pct, 20);
        assert_eq!(status.command, FanLevel::Low);

        let (status, _) = ctl.update(Input {
            now_ms: 1_000,
            temps_ready: true,
            temp_a_c_x16: Some(39 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.requested_pwm_pct, 15);
        assert_eq!(status.command, FanLevel::Low);

        let (status, _) = ctl.update(Input {
            now_ms: 1_500,
            temps_ready: true,
            temp_a_c_x16: Some(39 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.requested_pwm_pct, 10);
        assert_eq!(status.command, FanLevel::Low);

        let (status, _) = ctl.update(Input {
            now_ms: 2_000,
            temps_ready: true,
            temp_a_c_x16: Some(36 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 0,
        });
        assert_eq!(status.requested_pwm_pct, 0);
        assert_eq!(status.command, FanLevel::Off);
    }

    #[test]
    fn degrades_to_single_sensor_and_fails_safe_when_both_missing() {
        let mut ctl = Controller::new(cfg());

        let (status, _) = ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(42 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.temp_source, TempSource::TmpA);
        assert_eq!(status.command, FanLevel::Low);

        let (status, _) = ctl.update(Input {
            now_ms: 500,
            temps_ready: true,
            temp_a_c_x16: None,
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.requested_pwm_pct, 100);
        assert_eq!(status.command, FanLevel::High);
        assert_eq!(status.temp_source, TempSource::Missing);
    }

    #[test]
    fn latches_tach_fault_until_recovered() {
        let mut ctl = Controller::new(cfg());

        let (_, _) = ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(41 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 0,
        });

        let (status, _) = ctl.update(Input {
            now_ms: 2_100,
            temps_ready: true,
            temp_a_c_x16: Some(41 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 0,
        });
        assert!(status.tach_fault);
        assert_eq!(status.command, FanLevel::High);
        assert_eq!(status.pwm_pct, 100);

        let (status, _) = ctl.update(Input {
            now_ms: 2_130,
            temps_ready: true,
            temp_a_c_x16: Some(39 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });
        assert!(status.tach_fault);

        let (status, _) = ctl.update(Input {
            now_ms: 2_160,
            temps_ready: true,
            temp_a_c_x16: Some(39 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 1,
        });
        assert!(!status.tach_fault);
        assert_eq!(status.command, FanLevel::Low);
        assert_eq!(status.pwm_pct, 30);
    }

    #[test]
    fn disables_tach_watchdog_in_test_mode() {
        let mut test_cfg = cfg();
        test_cfg.tach_watchdog_enabled = false;
        let mut ctl = Controller::new(test_cfg);

        let (status, _) = ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(44 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 0,
        });
        assert!(!status.tach_fault);

        let (status, _) = ctl.update(Input {
            now_ms: 3_000,
            temps_ready: true,
            temp_a_c_x16: Some(44 * 16),
            temp_b_c_x16: None,
            temp_bms_c_x16: None,
            tach_pulse_count: 0,
        });
        assert!(!status.tach_fault);
        assert_eq!(status.command, FanLevel::Mid);
    }

    #[test]
    fn uses_bms_thermal_max_when_tmp_is_missing() {
        let mut ctl = Controller::new(cfg());

        let (status, _) = ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: None,
            temp_b_c_x16: None,
            temp_bms_c_x16: Some(42 * 16),
            tach_pulse_count: 1,
        });

        assert_eq!(status.temp_source, TempSource::Bms);
        assert_eq!(status.control_temp_c_x16, Some(42 * 16));
        assert_eq!(status.command, FanLevel::Low);
    }

    #[test]
    fn picks_max_across_tmp_and_bms_inputs() {
        let mut ctl = Controller::new(cfg());

        let (status, _) = ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(39 * 16),
            temp_b_c_x16: Some(41 * 16),
            temp_bms_c_x16: Some(43 * 16),
            tach_pulse_count: 1,
        });

        assert_eq!(status.temp_source, TempSource::Max);
        assert_eq!(status.control_temp_c_x16, Some(43 * 16));
        assert_eq!(status.requested_pwm_pct, 25);
    }
}
