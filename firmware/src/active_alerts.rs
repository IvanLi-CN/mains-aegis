use crate::audio::AudioCue;
use core::fmt::Write;

pub const ALERT_COUNT: usize = 9;

pub const fn mains_absent_active(previously_active: bool, mains_present: Option<bool>) -> bool {
    match mains_present {
        Some(false) => true,
        Some(true) => false,
        None => previously_active,
    }
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertId {
    MainsAbsentDc,
    HighStress,
    BatteryLowNoMains,
    BatteryLowWithMains,
    ShutdownProtection,
    IoOverVoltage,
    IoOverCurrent,
    ModuleFault,
    BatteryProtection,
}

impl AlertId {
    pub const ALL: [Self; ALERT_COUNT] = [
        Self::MainsAbsentDc,
        Self::HighStress,
        Self::BatteryLowNoMains,
        Self::BatteryLowWithMains,
        Self::ShutdownProtection,
        Self::IoOverVoltage,
        Self::IoOverCurrent,
        Self::ModuleFault,
        Self::BatteryProtection,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MainsAbsentDc => "mains_absent_dc",
            Self::HighStress => "high_stress",
            Self::BatteryLowNoMains => "battery_low_no_mains",
            Self::BatteryLowWithMains => "battery_low_with_mains",
            Self::ShutdownProtection => "shutdown_protection",
            Self::IoOverVoltage => "io_over_voltage",
            Self::IoOverCurrent => "io_over_current",
            Self::ModuleFault => "module_fault",
            Self::BatteryProtection => "battery_protection",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.as_str() == value)
    }

    pub const fn index(self) -> usize {
        match self {
            Self::MainsAbsentDc => 0,
            Self::HighStress => 1,
            Self::BatteryLowNoMains => 2,
            Self::BatteryLowWithMains => 3,
            Self::ShutdownProtection => 4,
            Self::IoOverVoltage => 5,
            Self::IoOverCurrent => 6,
            Self::ModuleFault => 7,
            Self::BatteryProtection => 8,
        }
    }

    pub const fn severity(self) -> AlertSeverity {
        match self {
            Self::MainsAbsentDc
            | Self::HighStress
            | Self::BatteryLowNoMains
            | Self::BatteryLowWithMains => AlertSeverity::Warning,
            _ => AlertSeverity::Critical,
        }
    }

    pub const fn audio_cue(self) -> AudioCue {
        match self {
            Self::MainsAbsentDc => AudioCue::MainsAbsentDc,
            Self::HighStress => AudioCue::HighStress,
            Self::BatteryLowNoMains => AudioCue::BatteryLowNoMains,
            Self::BatteryLowWithMains => AudioCue::BatteryLowWithMains,
            Self::ShutdownProtection => AudioCue::ShutdownProtection,
            Self::IoOverVoltage => AudioCue::IoOverVoltage,
            Self::IoOverCurrent => AudioCue::IoOverCurrent,
            Self::ModuleFault => AudioCue::ModuleFault,
            Self::BatteryProtection => AudioCue::BatteryProtection,
        }
    }

    pub const fn summary(self) -> &'static str {
        match self {
            Self::MainsAbsentDc => "RUNNING ON BATTERY",
            Self::HighStress => "CHECK THERMAL LOAD",
            Self::BatteryLowNoMains => "NO MAINS - REDUCE LOAD",
            Self::BatteryLowWithMains => "CHECK CHARGING PATH",
            Self::ShutdownProtection => "OUTPUT PROTECTION ACTIVE",
            Self::IoOverVoltage => "CHECK OUTPUT LOAD",
            Self::IoOverCurrent => "REDUCE OUTPUT LOAD",
            Self::ModuleFault => "CHECK DEVICE DIAGNOSTICS",
            Self::BatteryProtection => "CHECK BATTERY STATUS",
        }
    }
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertSeverity {
    Warning,
    Critical,
}

impl AlertSeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertSoundState {
    Audible,
    Muted,
    SystemSilent,
    PolicySilent,
}

impl AlertSoundState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Audible => "audible",
            Self::Muted => "muted",
            Self::SystemSilent => "system_silent",
            Self::PolicySilent => "policy_silent",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AlertSignals {
    active: [bool; ALERT_COUNT],
    policy_silent: [bool; ALERT_COUNT],
}

impl AlertSignals {
    pub fn set(&mut self, id: AlertId, active: bool) {
        self.active[id.index()] = active;
    }

    pub fn set_policy_silent(&mut self, id: AlertId, silent: bool) {
        self.policy_silent[id.index()] = silent;
    }
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveAlert {
    pub alert_id: AlertId,
    pub instance_id: u32,
    pub severity: AlertSeverity,
    pub sound: AlertSoundState,
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MuteResult {
    Muted,
    AlreadyMuted,
    Stale { current_instance_id: u32 },
    Inactive,
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    active: bool,
    instance_id: u32,
    muted: bool,
    policy_silent: bool,
}

impl Slot {
    const EMPTY: Self = Self {
        active: false,
        instance_id: 0,
        muted: false,
        policy_silent: false,
    };
}

pub struct ActiveAlerts {
    slots: [Slot; ALERT_COUNT],
    next_instance_id: u32,
}

impl ActiveAlerts {
    pub const fn new() -> Self {
        Self {
            slots: [Slot::EMPTY; ALERT_COUNT],
            next_instance_id: 1,
        }
    }

    pub fn update(&mut self, signals: AlertSignals) {
        for id in AlertId::ALL {
            let slot = &mut self.slots[id.index()];
            let active = signals.active[id.index()];
            if active && !slot.active {
                slot.instance_id = self.next_instance_id;
                self.next_instance_id = self.next_instance_id.wrapping_add(1).max(1);
                slot.muted = false;
            } else if !active && slot.active {
                slot.muted = false;
            }
            slot.active = active;
            slot.policy_silent = active && signals.policy_silent[id.index()];
        }
    }

    pub fn get(&self, id: AlertId, system_silent: bool) -> Option<ActiveAlert> {
        let slot = self.slots[id.index()];
        slot.active.then(|| ActiveAlert {
            alert_id: id,
            instance_id: slot.instance_id,
            severity: id.severity(),
            sound: effective_sound(slot, system_silent),
        })
    }

    pub fn for_each_active(&self, system_silent: bool, mut f: impl FnMut(ActiveAlert)) {
        for id in AlertId::ALL {
            if let Some(alert) = self.get(id, system_silent) {
                f(alert);
            }
        }
    }

    pub fn mute(&mut self, id: AlertId, instance_id: u32) -> MuteResult {
        let slot = &mut self.slots[id.index()];
        if !slot.active {
            return MuteResult::Inactive;
        }
        if slot.instance_id != instance_id {
            return MuteResult::Stale {
                current_instance_id: slot.instance_id,
            };
        }
        if slot.muted {
            return MuteResult::AlreadyMuted;
        }
        slot.muted = true;
        MuteResult::Muted
    }

    pub fn cue_should_play(&self, id: AlertId, system_silent: bool) -> bool {
        self.get(id, system_silent)
            .is_some_and(|alert| alert.sound == AlertSoundState::Audible)
    }

    pub fn is_policy_silent(&self, id: AlertId) -> bool {
        let slot = self.slots[id.index()];
        slot.active && slot.policy_silent
    }
}

impl Default for ActiveAlerts {
    fn default() -> Self {
        Self::new()
    }
}

fn effective_sound(slot: Slot, system_silent: bool) -> AlertSoundState {
    if slot.muted {
        AlertSoundState::Muted
    } else if system_silent {
        AlertSoundState::SystemSilent
    } else if slot.policy_silent {
        AlertSoundState::PolicySilent
    } else {
        AlertSoundState::Audible
    }
}

pub fn render_alerts_json<const N: usize>(
    out: &mut heapless::String<N>,
    alerts: &ActiveAlerts,
    system_silent: bool,
) {
    out.clear();
    let _ = out.push_str(r#"{"alerts":["#);
    let mut first = true;
    alerts.for_each_active(system_silent, |alert| {
        if !first {
            let _ = out.push(',');
        }
        first = false;
        let _ = write!(
            out,
            r#"{{"alert_id":"{}","instance_id":{},"severity":"{}","sound_state":"{}","summary":"{}"}}"#,
            alert.alert_id.as_str(),
            alert.instance_id,
            alert.severity.as_str(),
            alert.sound.as_str(),
            alert.alert_id.summary(),
        );
    });
    let _ = out.push_str("]}");
}

pub fn render_mute_result_json<const N: usize>(
    out: &mut heapless::String<N>,
    id: AlertId,
    instance_id: u32,
    result: MuteResult,
) {
    out.clear();
    match result {
        MuteResult::Muted | MuteResult::AlreadyMuted => {
            let result = if result == MuteResult::Muted {
                "muted"
            } else {
                "already_muted"
            };
            let _ = write!(
                out,
                r#"{{"ok":true,"alert_id":"{}","instance_id":{},"severity":"{}","sound_state":"muted","summary":"{}","result":"{}"}}"#,
                id.as_str(),
                instance_id,
                id.severity().as_str(),
                id.summary(),
                result
            );
        }
        MuteResult::Stale {
            current_instance_id,
        } => {
            let _ = write!(
                out,
                r#"{{"ok":false,"alert_id":"{}","instance_id":{},"result":"stale","current_instance_id":{}}}"#,
                id.as_str(),
                instance_id,
                current_instance_id
            );
        }
        MuteResult::Inactive => {
            let _ = write!(
                out,
                r#"{{"ok":false,"alert_id":"{}","instance_id":{},"result":"inactive"}}"#,
                id.as_str(),
                instance_id
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_the_nine_runtime_alerts() {
        assert_eq!(AlertId::ALL.len(), 9);
        for id in AlertId::ALL {
            assert_eq!(AlertId::parse(id.as_str()), Some(id));
        }
        assert_eq!(AlertId::parse("io_over_power"), None);
    }

    #[test]
    fn mains_absent_alert_holds_its_instance_while_telemetry_is_unknown() {
        assert!(mains_absent_active(false, Some(false)));
        assert!(mains_absent_active(true, None));
        assert!(!mains_absent_active(false, None));
        assert!(!mains_absent_active(true, Some(true)));
    }

    #[test]
    fn mute_only_applies_to_the_current_instance() {
        let mut alerts = ActiveAlerts::new();
        let mut signals = AlertSignals::default();
        signals.set(AlertId::HighStress, true);
        alerts.update(signals);
        let first = alerts.get(AlertId::HighStress, false).unwrap();
        assert_eq!(
            alerts.mute(first.alert_id, first.instance_id),
            MuteResult::Muted
        );
        assert!(!alerts.cue_should_play(AlertId::HighStress, false));

        signals.set(AlertId::HighStress, false);
        alerts.update(signals);
        signals.set(AlertId::HighStress, true);
        alerts.update(signals);
        let second = alerts.get(AlertId::HighStress, false).unwrap();
        assert_ne!(first.instance_id, second.instance_id);
        assert_eq!(
            alerts.mute(AlertId::HighStress, first.instance_id),
            MuteResult::Stale {
                current_instance_id: second.instance_id
            }
        );
        assert!(alerts.cue_should_play(AlertId::HighStress, false));
    }

    #[test]
    fn clearing_one_alert_does_not_change_another() {
        let mut alerts = ActiveAlerts::new();
        let mut signals = AlertSignals::default();
        signals.set(AlertId::HighStress, true);
        signals.set(AlertId::ModuleFault, true);
        alerts.update(signals);
        let module = alerts.get(AlertId::ModuleFault, false).unwrap();
        alerts.mute(module.alert_id, module.instance_id);

        signals.set(AlertId::HighStress, false);
        alerts.update(signals);
        assert!(alerts.get(AlertId::HighStress, false).is_none());
        assert_eq!(
            alerts.get(AlertId::ModuleFault, false).unwrap().sound,
            AlertSoundState::Muted
        );
    }

    #[test]
    fn reports_system_and_policy_silence_without_mutating_the_instance() {
        let mut alerts = ActiveAlerts::new();
        let mut signals = AlertSignals::default();
        signals.set(AlertId::MainsAbsentDc, true);
        signals.set_policy_silent(AlertId::MainsAbsentDc, true);
        alerts.update(signals);
        let instance = alerts
            .get(AlertId::MainsAbsentDc, false)
            .unwrap()
            .instance_id;
        assert_eq!(
            alerts.get(AlertId::MainsAbsentDc, false).unwrap().sound,
            AlertSoundState::PolicySilent
        );
        assert_eq!(
            alerts.get(AlertId::MainsAbsentDc, true).unwrap().sound,
            AlertSoundState::SystemSilent
        );
        assert_eq!(
            alerts
                .get(AlertId::MainsAbsentDc, false)
                .unwrap()
                .instance_id,
            instance
        );
    }

    #[test]
    fn inactive_and_duplicate_mutes_are_deterministic() {
        let mut alerts = ActiveAlerts::new();
        assert_eq!(alerts.mute(AlertId::IoOverCurrent, 1), MuteResult::Inactive);
        let mut signals = AlertSignals::default();
        signals.set(AlertId::IoOverCurrent, true);
        alerts.update(signals);
        let alert = alerts.get(AlertId::IoOverCurrent, false).unwrap();
        assert_eq!(
            alerts.mute(alert.alert_id, alert.instance_id),
            MuteResult::Muted
        );
        assert_eq!(
            alerts.mute(alert.alert_id, alert.instance_id),
            MuteResult::AlreadyMuted
        );
    }

    #[test]
    fn renders_stable_transport_json() {
        let mut alerts = ActiveAlerts::new();
        let mut signals = AlertSignals::default();
        signals.set(AlertId::ModuleFault, true);
        alerts.update(signals);
        let mut json = heapless::String::<512>::new();
        render_alerts_json(&mut json, &alerts, false);
        assert_eq!(
            json.as_str(),
            r#"{"alerts":[{"alert_id":"module_fault","instance_id":1,"severity":"critical","sound_state":"audible","summary":"CHECK DEVICE DIAGNOSTICS"}]}"#
        );

        render_mute_result_json(&mut json, AlertId::ModuleFault, 1, MuteResult::Muted);
        assert_eq!(
            json.as_str(),
            r#"{"ok":true,"alert_id":"module_fault","instance_id":1,"severity":"critical","sound_state":"muted","summary":"CHECK DEVICE DIAGNOSTICS","result":"muted"}"#
        );
    }
}
