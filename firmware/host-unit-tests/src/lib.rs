#![allow(dead_code)]

#[cfg(all(
    not(feature = "no-pps"),
    not(any(
        not(feature = "no-pd-sink-5v"),
        not(feature = "no-pd-sink-9v"),
        not(feature = "no-pd-sink-12v"),
        not(feature = "no-pd-sink-15v"),
        not(feature = "no-pd-sink-20v"),
    ))
))]
compile_error!(
    "PPS requires at least one enabled fixed PDO; clear at least one no-pd-sink-* feature"
);

extern crate self as esp_firmware;

#[no_mangle]
pub extern "Rust" fn _defmt_acquire() {}

#[no_mangle]
pub extern "Rust" fn _defmt_release() {}

#[no_mangle]
pub extern "Rust" fn _defmt_write(_bytes: &[u8]) {}

#[no_mangle]
pub extern "Rust" fn _defmt_timestamp(_fmt: defmt::Formatter<'_>) {}

#[no_mangle]
pub extern "Rust" fn _defmt_flush() {}

pub mod time {
    pub use std::time::{Duration, Instant};
}

pub mod ina3221 {
    pub use ina3221_async::*;
}

#[path = "../../src/bq25792.rs"]
pub mod bq25792;

#[path = "../../src/bq40z50.rs"]
pub mod bq40z50;

#[path = "../../src/display_pipeline.rs"]
pub mod display_pipeline;

#[path = "../../src/display_power.rs"]
pub mod display_power;

#[path = "../../src/fan.rs"]
pub mod fan;

#[path = "../../src/output_protection.rs"]
pub mod output_protection;

#[path = "../../src/output_state.rs"]
pub mod output_state;

#[path = "../../src/output_retry.rs"]
pub mod output_retry;

#[path = "../../src/tmp112.rs"]
pub mod tmp112;

#[path = "../../src/audio.rs"]
pub mod audio;

#[path = "../../src/front_panel_scene.rs"]
pub mod front_panel_scene;

#[path = "../../src/front_panel_logic.rs"]
pub mod front_panel_logic;

#[path = "../../src/net_types.rs"]
pub mod net_types;

#[path = "../../src/mdns_wire.rs"]
pub mod mdns_wire;

#[path = "../../src/net_contract.rs"]
pub mod net_contract;

#[path = "../../src/net_logic.rs"]
pub mod net_logic;

#[path = "../../src/net_bridge.rs"]
pub mod net_bridge;

#[path = "../../src/usb_cdc_protocol.rs"]
pub mod usb_cdc_protocol;

#[path = "../../build_support/wifi_env.rs"]
pub mod wifi_env;

pub mod usb_pd;

pub mod output {
    use crate::bq40z50;
    use crate::front_panel_scene::UpsMode;
    use crate::output_state::OutputGateReason;

    const CHARGER_INPUT_IBUS_MAX_MA: i16 = 5_000;
    const CHARGER_INPUT_VBUS_MAX_MV: u16 = 30_000;
    const VIN_MAINS_PRESENT_THRESHOLD_MV: u16 = 3_000;
    const VIN_MAINS_LATCH_FAILURE_LIMIT: u8 = 2;
    const CHARGER_INPUT_POWER_ANOMALY_W10: u32 = 2_000;

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct OutputRuntimeState {
        requested_outputs: pure::EnabledOutputs,
        active_outputs: pure::EnabledOutputs,
        recoverable_outputs: pure::EnabledOutputs,
        gate_reason: OutputGateReason,
    }

    impl OutputRuntimeState {
        pub const fn new(
            requested_outputs: pure::EnabledOutputs,
            active_outputs: pure::EnabledOutputs,
            recoverable_outputs: pure::EnabledOutputs,
            gate_reason: OutputGateReason,
        ) -> Self {
            Self {
                requested_outputs,
                active_outputs,
                recoverable_outputs,
                gate_reason,
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum AudioChargePhase {
        Unknown,
        NotCharging,
        Charging,
        Completed,
    }

    impl Default for AudioChargePhase {
        fn default() -> Self {
            Self::Unknown
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum AudioBatteryLowState {
        Unknown,
        Inactive,
        WithMains,
        NoMains,
    }

    impl Default for AudioBatteryLowState {
        fn default() -> Self {
            Self::Unknown
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum AudioMainsSource {
        #[default]
        Unknown,
        Vin,
        AggregateInputPresent,
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct StableMainsState {
        present: Option<bool>,
        source: AudioMainsSource,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ChargerInputSampleIssue {
        AdcNotReady,
        VbusMissing,
        VbusOutOfRange,
        IbusMissing,
        IbusOutOfRange,
    }

    impl ChargerInputSampleIssue {
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::AdcNotReady => "adc_not_ready",
                Self::VbusMissing => "vbus_missing",
                Self::VbusOutOfRange => "vbus_out_of_range",
                Self::IbusMissing => "ibus_missing",
                Self::IbusOutOfRange => "ibus_out_of_range",
            }
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ChargerInputPowerSample {
        raw_vbus_mv: Option<u16>,
        raw_ibus_ma: Option<i16>,
        ui_vbus_mv: Option<u16>,
        ui_ibus_ma: Option<i32>,
        raw_power_w10: Option<u32>,
        issue: Option<ChargerInputSampleIssue>,
    }

    #[derive(Clone, Copy)]
    pub struct Bq40z50Snapshot {
        battery_mode: u16,
        temp_k_x10: u16,
        vpack_mv: u16,
        current_ma: i16,
        rsoc_pct: u16,
        remcap: u16,
        fcc: u16,
        batt_status: u16,
        op_status: Option<u32>,
        da_status2: Option<bq40z50::DaStatus2>,
        filter_capacity: Option<bq40z50::FilterCapacity>,
        balance_config: Option<bq40z50::BalanceConfig>,
        afe_register: Option<bq40z50::AfeRegister>,
        cell_mv: [u16; 4],
    }

    fn normalize_charger_input_power_sample(
        input_present: bool,
        adc_ready: bool,
        raw_vbus_mv: Option<u16>,
        raw_ibus_ma: Option<i16>,
    ) -> ChargerInputPowerSample {
        let raw_power_w10 = match (raw_vbus_mv, raw_ibus_ma) {
            (Some(vbus_mv), Some(ibus_ma)) => {
                Some((vbus_mv as u32 * ibus_ma.unsigned_abs() as u32) / 100_000)
            }
            _ => None,
        };

        let mut sample = ChargerInputPowerSample {
            raw_vbus_mv,
            raw_ibus_ma,
            ui_vbus_mv: None,
            ui_ibus_ma: None,
            raw_power_w10,
            issue: None,
        };

        if !input_present {
            return sample;
        }

        if !adc_ready {
            sample.issue = Some(ChargerInputSampleIssue::AdcNotReady);
            return sample;
        }

        let vbus_mv = match raw_vbus_mv {
            Some(vbus_mv) if vbus_mv <= CHARGER_INPUT_VBUS_MAX_MV => vbus_mv,
            Some(_) => {
                sample.issue = Some(ChargerInputSampleIssue::VbusOutOfRange);
                return sample;
            }
            None => {
                sample.issue = Some(ChargerInputSampleIssue::VbusMissing);
                return sample;
            }
        };

        let ibus_ma = match raw_ibus_ma {
            Some(ibus_ma)
                if ibus_ma >= -CHARGER_INPUT_IBUS_MAX_MA
                    && ibus_ma <= CHARGER_INPUT_IBUS_MAX_MA =>
            {
                ibus_ma
            }
            Some(_) => {
                sample.issue = Some(ChargerInputSampleIssue::IbusOutOfRange);
                return sample;
            }
            None => {
                sample.issue = Some(ChargerInputSampleIssue::IbusMissing);
                return sample;
            }
        };

        sample.ui_vbus_mv = Some(vbus_mv);
        sample.ui_ibus_ma = Some(if ibus_ma <= 0 { 0 } else { i32::from(ibus_ma) });
        sample
    }

    fn mains_present_from_vin(vin_vbus_mv: Option<u16>) -> Option<bool> {
        vin_vbus_mv.map(|mv| mv >= VIN_MAINS_PRESENT_THRESHOLD_MV)
    }

    fn stable_mains_present(
        vin_mains_present: Option<bool>,
        vin_vbus_mv: Option<u16>,
        charger_present: Option<bool>,
    ) -> Option<bool> {
        stable_mains_state(vin_mains_present, vin_vbus_mv, charger_present).present
    }

    fn stable_mains_state(
        vin_mains_present: Option<bool>,
        vin_vbus_mv: Option<u16>,
        charger_present: Option<bool>,
    ) -> StableMainsState {
        if let Some(present) = mains_present_from_vin(vin_vbus_mv) {
            return StableMainsState {
                present: Some(present),
                source: AudioMainsSource::Vin,
            };
        }
        if let Some(present) = vin_mains_present {
            return StableMainsState {
                present: Some(present),
                source: AudioMainsSource::Vin,
            };
        }
        if let Some(present) = charger_present {
            return StableMainsState {
                present: Some(present),
                source: AudioMainsSource::AggregateInputPresent,
            };
        }
        StableMainsState::default()
    }

    fn mains_present_edge(prev: StableMainsState, next: StableMainsState) -> Option<bool> {
        if prev.present.is_some() && next.present.is_some() && prev.present != next.present {
            next.present
        } else {
            None
        }
    }

    fn discharge_authorization_input_ready(
        mains_present: Option<bool>,
        charger_present: Option<bool>,
    ) -> bool {
        charger_present == Some(true) || mains_present == Some(true)
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RuntimeChargeOverride {
        allow_charge: bool,
        policy_status_text: &'static str,
        policy_notice_text: &'static str,
    }

    const fn runtime_charge_override(mode: UpsMode) -> Option<RuntimeChargeOverride> {
        match mode {
            UpsMode::Supplement => Some(RuntimeChargeOverride {
                allow_charge: false,
                policy_status_text: "LOAD",
                policy_notice_text: "runtime_assist_no_charge",
            }),
            UpsMode::Backup => Some(RuntimeChargeOverride {
                allow_charge: false,
                policy_status_text: "NOAC",
                policy_notice_text: "runtime_backup_no_charge",
            }),
            UpsMode::Standby | UpsMode::Off => None,
        }
    }

    #[test]
    fn runtime_charge_override_blocks_charging_in_assist_and_backup_only() {
        assert_eq!(
            runtime_charge_override(UpsMode::Supplement),
            Some(RuntimeChargeOverride {
                allow_charge: false,
                policy_status_text: "LOAD",
                policy_notice_text: "runtime_assist_no_charge",
            })
        );
        assert_eq!(
            runtime_charge_override(UpsMode::Backup),
            Some(RuntimeChargeOverride {
                allow_charge: false,
                policy_status_text: "NOAC",
                policy_notice_text: "runtime_backup_no_charge",
            })
        );
        assert_eq!(runtime_charge_override(UpsMode::Standby), None);
        assert_eq!(runtime_charge_override(UpsMode::Off), None);
    }

    #[test]
    fn runtime_mode_tracker_resets_assist_streak_after_confirmed_input_loss() {
        let mut tracker = RuntimeModeTracker::new(UpsMode::Standby);

        assert_eq!(
            tracker.update(Some(true), Some(120), true, Some(1)),
            UpsMode::Standby
        );
        assert_eq!(
            tracker.update(Some(false), None, false, None),
            UpsMode::Backup
        );
        assert_eq!(
            tracker.update(Some(true), Some(120), true, Some(2)),
            UpsMode::Standby
        );
        assert_eq!(
            tracker.update(Some(true), Some(120), true, Some(3)),
            UpsMode::Supplement
        );
    }

    fn record_vin_sample_failure(vin_mains_present: &mut Option<bool>, missing_streak: &mut u8) {
        *missing_streak = missing_streak.saturating_add(1);
        if *missing_streak >= VIN_MAINS_LATCH_FAILURE_LIMIT {
            *vin_mains_present = None;
        }
    }

    fn mark_vin_telemetry_unavailable(
        telemetry_include_vin_ch3: bool,
        vin_vbus_mv: &mut Option<u16>,
        vin_iin_ma: &mut Option<i32>,
        vin_mains_present: &mut Option<bool>,
        missing_streak: &mut u8,
    ) {
        *vin_vbus_mv = None;
        *vin_iin_ma = None;
        if telemetry_include_vin_ch3 {
            record_vin_sample_failure(vin_mains_present, missing_streak);
        } else {
            *vin_mains_present = None;
            *missing_streak = 0;
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RuntimeModeTracker {
        mains_mode: UpsMode,
        assist_enter_streak: u8,
        standby_enter_streak: u8,
        last_tps_total_iout_sample_seq: Option<u32>,
    }

    impl RuntimeModeTracker {
        const ASSIST_ENTER_MA: i32 = 100;
        const STANDBY_EXIT_MA: i32 = 50;
        const REQUIRED_SAMPLES: u8 = 2;

        const fn new(initial_mode: UpsMode) -> Self {
            let mains_mode = match initial_mode {
                UpsMode::Backup => UpsMode::Backup,
                _ => UpsMode::Standby,
            };
            Self {
                mains_mode,
                assist_enter_streak: 0,
                standby_enter_streak: 0,
                last_tps_total_iout_sample_seq: None,
            }
        }

        fn update(
            &mut self,
            mains_present: Option<bool>,
            tps_total_iout_ma: Option<i32>,
            tps_total_iout_fresh: bool,
            tps_total_iout_sample_seq: Option<u32>,
        ) -> UpsMode {
            match mains_present {
                Some(false) => {
                    self.mains_mode = UpsMode::Backup;
                    self.assist_enter_streak = 0;
                    self.standby_enter_streak = 0;
                    if tps_total_iout_fresh {
                        self.last_tps_total_iout_sample_seq = tps_total_iout_sample_seq;
                    }
                    UpsMode::Backup
                }
                Some(true) => {
                    let sample_is_new = tps_total_iout_fresh
                        && tps_total_iout_sample_seq.is_some()
                        && tps_total_iout_sample_seq != self.last_tps_total_iout_sample_seq;
                    if sample_is_new {
                        self.last_tps_total_iout_sample_seq = tps_total_iout_sample_seq;
                        match tps_total_iout_ma {
                            Some(total_iout_ma) if total_iout_ma >= Self::ASSIST_ENTER_MA => {
                                self.assist_enter_streak =
                                    self.assist_enter_streak.saturating_add(1);
                                self.standby_enter_streak = 0;
                                if self.assist_enter_streak >= Self::REQUIRED_SAMPLES {
                                    self.mains_mode = UpsMode::Supplement;
                                }
                            }
                            Some(total_iout_ma) if total_iout_ma <= Self::STANDBY_EXIT_MA => {
                                self.standby_enter_streak =
                                    self.standby_enter_streak.saturating_add(1);
                                self.assist_enter_streak = 0;
                                if self.standby_enter_streak >= Self::REQUIRED_SAMPLES {
                                    self.mains_mode = UpsMode::Standby;
                                }
                            }
                            Some(_) => {
                                self.assist_enter_streak = 0;
                                self.standby_enter_streak = 0;
                            }
                            None => {}
                        }
                    }
                    if matches!(self.mains_mode, UpsMode::Backup) {
                        self.mains_mode = UpsMode::Standby;
                    }
                    self.mains_mode
                }
                None => self.mains_mode,
            }
        }
    }

    pub mod channel {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/output/channel.rs"
        ));
    }

    pub mod pure {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/output/pure.rs"
        ));
    }
}
