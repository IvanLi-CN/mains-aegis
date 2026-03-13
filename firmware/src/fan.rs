#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanLevel {
    Off,
    Mid,
    High,
}

impl FanLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Mid => "mid",
            Self::High => "high",
        }
    }

    pub const fn enabled(self) -> bool {
        !matches!(self, Self::Off)
    }

    pub const fn pwm_pct(self, mid_pwm_pct: u8) -> u8 {
        match self {
            Self::Off => 0,
            Self::Mid => mid_pwm_pct,
            Self::High => 100,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempSource {
    Pending,
    Missing,
    TmpA,
    TmpB,
    Max,
}

impl TempSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Missing => "missing",
            Self::TmpA => "tmp_a",
            Self::TmpB => "tmp_b",
            Self::Max => "max",
        }
    }

    pub const fn has_control_temp(self) -> bool {
        matches!(self, Self::TmpA | Self::TmpB | Self::Max)
    }

    pub const fn is_degraded(self) -> bool {
        matches!(self, Self::Missing | Self::TmpA | Self::TmpB)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub low_temp_c_x16: i16,
    pub high_temp_c_x16: i16,
    pub hysteresis_c_x16: i16,
    pub cooldown_ms: u64,
    pub tach_timeout_ms: u64,
    pub mid_pwm_pct: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub now_ms: u64,
    pub temps_ready: bool,
    pub temp_a_c_x16: Option<i16>,
    pub temp_b_c_x16: Option<i16>,
    pub tach_pulse_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub command: FanLevel,
    pub thermal_level: FanLevel,
    pub temp_source: TempSource,
    pub control_temp_c_x16: Option<i16>,
    pub tach_fault: bool,
    pub tach_pulse_seen_recently: bool,
    pub cooldown_active: bool,
}

impl Status {
    pub const fn pwm_pct(self, mid_pwm_pct: u8) -> u8 {
        self.command.pwm_pct(mid_pwm_pct)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Events {
    pub command_changed: bool,
    pub temp_source_changed: bool,
    pub tach_fault_changed: bool,
}

pub struct Controller {
    cfg: Config,
    thermal_level: FanLevel,
    cooldown_until_ms: Option<u64>,
    last_tach_seen_ms: Option<u64>,
    status: Status,
}

impl Controller {
    pub const fn new(cfg: Config) -> Self {
        Self {
            cfg,
            thermal_level: FanLevel::Off,
            cooldown_until_ms: None,
            last_tach_seen_ms: None,
            status: Status {
                command: FanLevel::Off,
                thermal_level: FanLevel::Off,
                temp_source: TempSource::Pending,
                control_temp_c_x16: None,
                tach_fault: false,
                tach_pulse_seen_recently: false,
                cooldown_active: false,
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
        let (control_temp_c_x16, temp_source) =
            select_control_temp(input.temps_ready, input.temp_a_c_x16, input.temp_b_c_x16);

        let prev_temp_available = prev.temp_source.has_control_temp();
        self.thermal_level = match temp_source {
            TempSource::Pending => FanLevel::Off,
            TempSource::Missing => FanLevel::High,
            _ => classify_thermal_level(
                control_temp_c_x16.expect("control temp must exist when source is present"),
                if prev_temp_available {
                    self.thermal_level
                } else {
                    FanLevel::Off
                },
                self.cfg,
            ),
        };

        let mut cooldown_active = false;
        let mut desired_command = self.thermal_level;
        if desired_command == FanLevel::Off {
            if self.cooldown_until_ms.is_none() && prev.thermal_level != FanLevel::Off {
                self.cooldown_until_ms = Some(input.now_ms.saturating_add(self.cfg.cooldown_ms));
            }
            if let Some(until_ms) = self.cooldown_until_ms {
                if input.now_ms < until_ms {
                    desired_command = FanLevel::Mid;
                    cooldown_active = true;
                } else {
                    self.cooldown_until_ms = None;
                }
            }
        } else {
            self.cooldown_until_ms = None;
        }

        let expecting_tach = desired_command.enabled();
        let mut tach_fault = prev.tach_fault;
        if input.tach_pulse_count > 0 {
            self.last_tach_seen_ms = Some(input.now_ms);
            tach_fault = false;
        } else if expecting_tach {
            if !prev.command.enabled() && !prev.tach_fault {
                self.last_tach_seen_ms = Some(input.now_ms);
            }
            if let Some(last_seen_ms) = self.last_tach_seen_ms {
                if input.now_ms.saturating_sub(last_seen_ms) >= self.cfg.tach_timeout_ms {
                    tach_fault = true;
                }
            }
        }

        let command = if tach_fault {
            FanLevel::High
        } else {
            desired_command
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
            command,
            thermal_level: self.thermal_level,
            temp_source,
            control_temp_c_x16,
            tach_fault,
            tach_pulse_seen_recently,
            cooldown_active,
        };

        (
            self.status,
            Events {
                command_changed: self.status.command != prev.command,
                temp_source_changed: self.status.temp_source != prev.temp_source,
                tach_fault_changed: self.status.tach_fault != prev.tach_fault,
            },
        )
    }
}

fn select_control_temp(
    temps_ready: bool,
    temp_a_c_x16: Option<i16>,
    temp_b_c_x16: Option<i16>,
) -> (Option<i16>, TempSource) {
    if !temps_ready {
        return (None, TempSource::Pending);
    }

    match (temp_a_c_x16, temp_b_c_x16) {
        (Some(a), Some(b)) => (Some(a.max(b)), TempSource::Max),
        (Some(a), None) => (Some(a), TempSource::TmpA),
        (None, Some(b)) => (Some(b), TempSource::TmpB),
        (None, None) => (None, TempSource::Missing),
    }
}

fn classify_thermal_level(temp_c_x16: i16, prev: FanLevel, cfg: Config) -> FanLevel {
    let low_fall = cfg.low_temp_c_x16 - cfg.hysteresis_c_x16;
    let high_fall = cfg.high_temp_c_x16 - cfg.hysteresis_c_x16;

    match prev {
        FanLevel::Off => {
            if temp_c_x16 >= cfg.high_temp_c_x16 {
                FanLevel::High
            } else if temp_c_x16 >= cfg.low_temp_c_x16 {
                FanLevel::Mid
            } else {
                FanLevel::Off
            }
        }
        FanLevel::Mid => {
            if temp_c_x16 >= cfg.high_temp_c_x16 {
                FanLevel::High
            } else if temp_c_x16 < low_fall {
                FanLevel::Off
            } else {
                FanLevel::Mid
            }
        }
        FanLevel::High => {
            if temp_c_x16 < high_fall {
                if temp_c_x16 < low_fall {
                    FanLevel::Off
                } else {
                    FanLevel::Mid
                }
            } else {
                FanLevel::High
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, Controller, FanLevel, Input, TempSource};

    fn cfg() -> Config {
        Config {
            low_temp_c_x16: 40 * 16,
            high_temp_c_x16: 50 * 16,
            hysteresis_c_x16: 3 * 16,
            cooldown_ms: 10_000,
            tach_timeout_ms: 2_000,
            mid_pwm_pct: 60,
        }
    }

    #[test]
    fn tracks_three_thermal_bands() {
        let mut ctl = Controller::new(cfg());

        let (status, _) = ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(39 * 16),
            temp_b_c_x16: Some(38 * 16),
            tach_pulse_count: 0,
        });
        assert_eq!(status.command, FanLevel::Off);

        let (status, _) = ctl.update(Input {
            now_ms: 100,
            temps_ready: true,
            temp_a_c_x16: Some(40 * 16),
            temp_b_c_x16: Some(39 * 16),
            tach_pulse_count: 1,
        });
        assert_eq!(status.command, FanLevel::Mid);
        assert_eq!(status.temp_source, TempSource::Max);

        let (status, _) = ctl.update(Input {
            now_ms: 200,
            temps_ready: true,
            temp_a_c_x16: Some(50 * 16),
            temp_b_c_x16: Some(47 * 16),
            tach_pulse_count: 1,
        });
        assert_eq!(status.command, FanLevel::High);
    }

    #[test]
    fn applies_hysteresis_near_thresholds() {
        let mut ctl = Controller::new(cfg());
        ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(41 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 1,
        });

        let (status, _) = ctl.update(Input {
            now_ms: 100,
            temps_ready: true,
            temp_a_c_x16: Some(38 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.command, FanLevel::Mid);

        let (status, _) = ctl.update(Input {
            now_ms: 200,
            temps_ready: true,
            temp_a_c_x16: Some(36 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.command, FanLevel::Mid);
        assert!(status.cooldown_active);
    }

    #[test]
    fn keeps_low_speed_during_cooldown_then_stops() {
        let mut ctl = Controller::new(cfg());
        ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(52 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 1,
        });

        let (status, _) = ctl.update(Input {
            now_ms: 100,
            temps_ready: true,
            temp_a_c_x16: Some(35 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.command, FanLevel::Mid);
        assert!(status.cooldown_active);

        let (status, _) = ctl.update(Input {
            now_ms: 10_200,
            temps_ready: true,
            temp_a_c_x16: Some(35 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 0,
        });
        assert_eq!(status.command, FanLevel::Off);
        assert!(!status.cooldown_active);
    }

    #[test]
    fn degrades_to_single_sensor_and_fails_safe_when_both_missing() {
        let mut ctl = Controller::new(cfg());

        let (status, _) = ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(42 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.command, FanLevel::Mid);
        assert_eq!(status.temp_source, TempSource::TmpA);

        let (status, _) = ctl.update(Input {
            now_ms: 100,
            temps_ready: true,
            temp_a_c_x16: None,
            temp_b_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.command, FanLevel::High);
        assert_eq!(status.temp_source, TempSource::Missing);
    }

    #[test]
    fn escalates_on_tach_timeout_and_recovers_on_pulse() {
        let mut ctl = Controller::new(cfg());
        ctl.update(Input {
            now_ms: 0,
            temps_ready: true,
            temp_a_c_x16: Some(42 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 1,
        });

        let (status, _) = ctl.update(Input {
            now_ms: 2_100,
            temps_ready: true,
            temp_a_c_x16: Some(42 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 0,
        });
        assert_eq!(status.command, FanLevel::High);
        assert!(status.tach_fault);

        let (status, _) = ctl.update(Input {
            now_ms: 2_150,
            temps_ready: true,
            temp_a_c_x16: Some(35 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 0,
        });
        assert_eq!(status.command, FanLevel::High);
        assert!(status.tach_fault);

        let (status, _) = ctl.update(Input {
            now_ms: 2_200,
            temps_ready: true,
            temp_a_c_x16: Some(42 * 16),
            temp_b_c_x16: None,
            tach_pulse_count: 1,
        });
        assert_eq!(status.command, FanLevel::Mid);
        assert!(!status.tach_fault);
    }
}
