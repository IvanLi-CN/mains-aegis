use crate::front_panel_scene::{
    SelfCheckCommState, TpsTestChargerSnapshot, TpsTestOutputSnapshot, TpsTestUiSnapshot,
    TpsTestVoutProfile,
};
use crate::tps55288_test::{
    configure_output, force_disable_output, i2c_error_kind, ina_error_kind,
    read_telemetry_snapshot, OutputChannel,
};
use esp_firmware::bq25792;
use esp_firmware::ina3221;
use esp_firmware::tmp112;
use esp_hal::gpio::Flex;
use esp_hal::i2c::master::I2c;
use esp_hal::time::{Duration, Instant};
use esp_hal::Blocking;

const INA_REINIT_RESET_CFG: u16 = 0x8000;
const RETRY_BACKOFF: Duration = Duration::from_secs(5);
const TMP112_OUT_A_ADDR: u8 = 0x48;
const TMP112_OUT_B_ADDR: u8 = 0x49;
const TMP112_THIGH_C_X16: i16 = 50 * 16;
const TMP112_TLOW_C_X16: i16 = 40 * 16;

pub const TEST_CHARGER_ENABLE: bool = false;
pub const TEST_CHARGE_VREG_MV: u16 = 16_800;
pub const TEST_CHARGE_ICHG_MA: u16 = 200;
pub const TEST_INPUT_LIMIT_MA: u16 = 500;
pub const TEST_OUT_A_OE: bool = false;
pub const TEST_OUT_B_OE: bool = false;
pub const TEST_VOUT_PROFILE: TpsTestVoutProfile = TpsTestVoutProfile::V5;
pub const TEST_ILIMIT_MA: u16 = 3_500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedTestProfile {
    pub charger_enable: bool,
    pub charge_vreg_mv: u16,
    pub charge_ichg_ma: u16,
    pub input_limit_ma: u16,
    pub out_a_oe: bool,
    pub out_b_oe: bool,
    pub vout_profile: TpsTestVoutProfile,
    pub ilimit_ma: u16,
}

pub const TEST_PROFILE: FixedTestProfile = FixedTestProfile {
    charger_enable: TEST_CHARGER_ENABLE,
    charge_vreg_mv: TEST_CHARGE_VREG_MV,
    charge_ichg_ma: TEST_CHARGE_ICHG_MA,
    input_limit_ma: TEST_INPUT_LIMIT_MA,
    out_a_oe: TEST_OUT_A_OE,
    out_b_oe: TEST_OUT_B_OE,
    vout_profile: TEST_VOUT_PROFILE,
    ilimit_ma: TEST_ILIMIT_MA,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OutputRuntimeState {
    requested_enabled: bool,
    applied: bool,
    retry_at: Option<Instant>,
    retry_reason: Option<&'static str>,
    terminal_fault: Option<&'static str>,
    comm_state: SelfCheckCommState,
    actual_enabled: Option<bool>,
    vset_mv: Option<u16>,
    vbus_mv: Option<u16>,
    iout_ma: Option<i32>,
    temp_c_x16: Option<i16>,
    status_bits: Option<u8>,
}

impl OutputRuntimeState {
    const fn new(requested_enabled: bool) -> Self {
        Self {
            requested_enabled,
            applied: false,
            retry_at: None,
            retry_reason: None,
            terminal_fault: None,
            comm_state: SelfCheckCommState::Pending,
            actual_enabled: None,
            vset_mv: None,
            vbus_mv: None,
            iout_ma: None,
            temp_c_x16: None,
            status_bits: None,
        }
    }

    const fn fault_text(&self) -> Option<&'static str> {
        if let Some(fault) = self.terminal_fault {
            Some(fault)
        } else {
            self.retry_reason
        }
    }

    const fn snapshot(&self) -> TpsTestOutputSnapshot {
        TpsTestOutputSnapshot {
            requested_enabled: self.requested_enabled,
            actual_enabled: self.actual_enabled,
            comm_state: self.comm_state,
            vset_mv: self.vset_mv,
            vbus_mv: self.vbus_mv,
            iout_ma: self.iout_ma,
            temp_c_x16: self.temp_c_x16,
            status_bits: self.status_bits,
            fault: self.fault_text(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChargerRuntimeState {
    comm_state: SelfCheckCommState,
    actual_enabled: bool,
    input_present: Option<bool>,
    vbat_present: Option<bool>,
    vbus_mv: Option<u16>,
    ibus_ma: Option<i32>,
    vbat_mv: Option<u16>,
    ichg_ma: Option<u16>,
    status: &'static str,
    fault: Option<&'static str>,
}

impl ChargerRuntimeState {
    const fn new() -> Self {
        Self {
            comm_state: SelfCheckCommState::Pending,
            actual_enabled: false,
            input_present: None,
            vbat_present: None,
            vbus_mv: None,
            ibus_ma: None,
            vbat_mv: None,
            ichg_ma: Some(TEST_CHARGE_ICHG_MA),
            status: "PEND",
            fault: None,
        }
    }

    const fn snapshot(&self) -> TpsTestChargerSnapshot {
        TpsTestChargerSnapshot {
            requested_enabled: TEST_CHARGER_ENABLE,
            actual_enabled: self.actual_enabled,
            comm_state: self.comm_state,
            input_present: self.input_present,
            vbat_present: self.vbat_present,
            vbus_mv: self.vbus_mv,
            ibus_ma: self.ibus_ma,
            vbat_mv: self.vbat_mv,
            ichg_ma: self.ichg_ma,
            status: self.status,
            fault: self.fault,
        }
    }
}

pub struct TpsTestRuntime {
    build_profile: &'static str,
    build_id: &'static str,
    i2c: I2c<'static, Blocking>,
    chg_ce: Flex<'static>,
    chg_ilim_hiz_brk: Flex<'static>,
    therm_kill: Flex<'static>,
    ina_ready: bool,
    ina_retry_at: Option<Instant>,
    tmp_config_mask: u8,
    tmp_retry_at: Option<Instant>,
    therm_kill_latched: bool,
    charger: ChargerRuntimeState,
    out_a: OutputRuntimeState,
    out_b: OutputRuntimeState,
}

impl TpsTestRuntime {
    pub fn new(
        build_profile: &'static str,
        build_id: &'static str,
        i2c: I2c<'static, Blocking>,
        chg_ce: Flex<'static>,
        chg_ilim_hiz_brk: Flex<'static>,
        therm_kill: Flex<'static>,
    ) -> Self {
        Self {
            build_profile,
            build_id,
            i2c,
            chg_ce,
            chg_ilim_hiz_brk,
            therm_kill,
            ina_ready: false,
            ina_retry_at: None,
            tmp_config_mask: 0,
            tmp_retry_at: None,
            therm_kill_latched: false,
            charger: ChargerRuntimeState::new(),
            out_a: OutputRuntimeState::new(TEST_OUT_A_OE),
            out_b: OutputRuntimeState::new(TEST_OUT_B_OE),
        }
    }

    pub fn tick(&mut self, now: Instant) -> TpsTestUiSnapshot {
        self.ensure_tmp_alerts(now);
        self.ensure_ina_ready(now);
        self.sample_therm_kill();
        self.poll_charger(now);

        let target_vout_mv = TEST_VOUT_PROFILE.target_mv();
        step_output(
            &mut self.i2c,
            now,
            self.ina_ready,
            self.therm_kill_latched,
            target_vout_mv,
            TEST_ILIMIT_MA,
            OutputChannel::OutA,
            &mut self.out_a,
        );
        step_output(
            &mut self.i2c,
            now,
            self.ina_ready,
            self.therm_kill_latched,
            target_vout_mv,
            TEST_ILIMIT_MA,
            OutputChannel::OutB,
            &mut self.out_b,
        );

        self.snapshot()
    }

    fn ensure_tmp_alerts(&mut self, now: Instant) {
        if self.tmp_config_mask == 0b11 && self.tmp_retry_at.is_none() {
            return;
        }
        if matches!(self.tmp_retry_at, Some(deadline) if now < deadline) {
            return;
        }

        let cfg = tmp112::AlertConfig {
            t_high_c_x16: TMP112_THIGH_C_X16,
            t_low_c_x16: TMP112_TLOW_C_X16,
            fault_queue: tmp112::FaultQueue::F4,
            conversion_rate: tmp112::ConversionRate::Hz1,
        };

        let mut configured_mask = self.tmp_config_mask;
        for (bit, addr) in [(0b01, TMP112_OUT_A_ADDR), (0b10, TMP112_OUT_B_ADDR)] {
            if (configured_mask & bit) != 0 {
                continue;
            }
            match tmp112::program_alert_config(&mut self.i2c, addr, cfg) {
                Ok(_) => {
                    configured_mask |= bit;
                    defmt::info!("tps-test: tmp112 configured addr=0x{=u8:x}", addr);
                }
                Err(err) => {
                    defmt::warn!(
                        "tps-test: tmp112 config err addr=0x{=u8:x} err={}",
                        addr,
                        i2c_error_kind(err)
                    );
                }
            }
        }
        self.tmp_config_mask = configured_mask;
        if self.tmp_config_mask != 0b11 {
            self.tmp_retry_at = Some(now + RETRY_BACKOFF);
        } else {
            self.tmp_retry_at = None;
        }
    }

    fn ensure_ina_ready(&mut self, now: Instant) {
        if self.ina_ready {
            return;
        }
        if matches!(self.ina_retry_at, Some(deadline) if now < deadline) {
            return;
        }

        let _ = ina3221::init_with_config(&mut self.i2c, INA_REINIT_RESET_CFG).map_err(|err| {
            defmt::warn!("tps-test: ina reset err={}", ina_error_kind(err));
        });
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(2) {}

        match ina3221::init_with_config(&mut self.i2c, ina3221::CONFIG_VALUE_CH12) {
            Ok(()) => {
                self.ina_ready = true;
                self.ina_retry_at = None;
                defmt::info!(
                    "tps-test: ina ready cfg=0x{=u16:x}",
                    ina3221::CONFIG_VALUE_CH12
                );
            }
            Err(err) => {
                self.ina_ready = false;
                self.ina_retry_at = Some(now + RETRY_BACKOFF);
                defmt::error!("tps-test: ina init err={}", ina_error_kind(err));
            }
        }
    }

    fn sample_therm_kill(&mut self) {
        if self.therm_kill.is_low() {
            self.therm_kill_latched = true;
        }
    }

    fn poll_charger(&mut self, now: Instant) {
        let _ = self.chg_ilim_hiz_brk.set_low();

        let _ = match bq25792::ensure_ship_fet_path_enabled(&mut self.i2c) {
            Ok(state) => {
                defmt::debug!(
                    "tps-test: charger ship path ctrl5_before=0x{=u8:x} ctrl5_after=0x{=u8:x} mode_before={=u8} mode_after={=u8}",
                    state.ctrl5_before,
                    state.ctrl5_after,
                    state.ship.sdrv_ctrl_before,
                    state.ship.sdrv_ctrl_after
                );
                Ok(())
            }
            Err(err) => {
                self.mark_charger_comm_failed("ship_path", err);
                Err(())
            }
        };
        if self.charger.comm_state == SelfCheckCommState::Err && self.charger.fault == Some("COMM")
        {
            return;
        }

        if let Err(err) = bq25792::ensure_watchdog_disabled(&mut self.i2c) {
            self.mark_charger_comm_failed("watchdog", err);
            return;
        }

        let ctrl0 = match bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0) {
            Ok(value) => value,
            Err(err) => {
                self.mark_charger_comm_failed("ctrl0_read", err);
                return;
            }
        };

        let mut st = [0u8; 5];
        if let Err(err) =
            bq25792::read_block(&mut self.i2c, bq25792::reg::CHARGER_STATUS_0, &mut st)
        {
            self.mark_charger_comm_failed("status_read", err);
            return;
        }
        let mut fault = [0u8; 2];
        if let Err(err) =
            bq25792::read_block(&mut self.i2c, bq25792::reg::FAULT_STATUS_0, &mut fault)
        {
            self.mark_charger_comm_failed("fault_read", err);
            return;
        }

        let status0 = st[0];
        let status1 = st[1];
        let status2 = st[2];
        let status3 = st[3];
        let status4 = st[4];
        let fault0 = fault[0];
        let fault1 = fault[1];

        let input_present = (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::PG_STAT) != 0;
        let vbat_present = (status2 & bq25792::status2::VBAT_PRESENT_STAT) != 0;
        let ts_cold = (status4 & bq25792::status4::TS_COLD_STAT) != 0;
        let ts_hot = (status4 & bq25792::status4::TS_HOT_STAT) != 0;
        let charger_fault = fault0 != 0 || fault1 != 0 || ts_cold || ts_hot;

        let adc_state = bq25792::ensure_adc_power_path(&mut self.i2c).ok();
        let adc_ready = adc_state
            .map(|state| bq25792::power_path_adc_ready(state, status3))
            .unwrap_or(false);
        let vbus_mv = if adc_ready {
            bq25792::read_u16(&mut self.i2c, bq25792::reg::VBUS_ADC).ok()
        } else {
            None
        };
        let ibus_ma = if adc_ready {
            bq25792::read_i16(&mut self.i2c, bq25792::reg::IBUS_ADC)
                .ok()
                .map(i32::from)
        } else {
            None
        };
        let vbat_mv = if adc_ready {
            bq25792::read_u16(&mut self.i2c, bq25792::reg::VBAT_ADC).ok()
        } else {
            None
        };

        let allow_charge =
            TEST_CHARGER_ENABLE && input_present && !charger_fault && !self.therm_kill_latched;

        if allow_charge {
            if let Err(err) =
                bq25792::set_charge_voltage_limit_mv(&mut self.i2c, TEST_CHARGE_VREG_MV)
            {
                self.mark_charger_comm_failed("vreg_write", err);
                return;
            }
            if let Err(err) =
                bq25792::set_charge_current_limit_ma(&mut self.i2c, TEST_CHARGE_ICHG_MA)
            {
                self.mark_charger_comm_failed("ichg_write", err);
                return;
            }
            if let Err(err) =
                bq25792::set_input_current_limit_ma(&mut self.i2c, TEST_INPUT_LIMIT_MA)
            {
                self.mark_charger_comm_failed("iindpm_write", err);
                return;
            }
            if let Err(err) = bq25792::write_u8(
                &mut self.i2c,
                bq25792::reg::CHARGER_CONTROL_0,
                (ctrl0 | bq25792::ctrl0::EN_CHG) & !bq25792::ctrl0::EN_HIZ,
            ) {
                self.mark_charger_comm_failed("ctrl0_write", err);
                return;
            }
            self.chg_ce.set_low();
        } else {
            self.chg_ce.set_high();
        }

        self.charger.actual_enabled = allow_charge;
        self.charger.comm_state = if charger_fault {
            SelfCheckCommState::Warn
        } else {
            SelfCheckCommState::Ok
        };
        self.charger.input_present = Some(input_present);
        self.charger.vbat_present = Some(vbat_present);
        self.charger.vbus_mv = vbus_mv;
        self.charger.ibus_ma = ibus_ma;
        self.charger.vbat_mv = vbat_mv;
        self.charger.ichg_ma = Some(TEST_CHARGE_ICHG_MA);
        self.charger.status =
            charger_status_text(charger_fault, input_present, allow_charge, status1);
        self.charger.fault = decode_charger_fault(
            charger_fault,
            input_present,
            ts_cold,
            ts_hot,
            fault0,
            fault1,
        );

        if !allow_charge {
            self.chg_ce.set_high();
        }

        defmt::info!(
            "tps-test: charger status={} actual={=bool} input_present={=bool} vbat_present={=bool} therm_latched={=bool} vbus_mv={=?} ibus_ma={=?} vbat_mv={=?} st0=0x{=u8:x} st1=0x{=u8:x} st2=0x{=u8:x} st3=0x{=u8:x} st4=0x{=u8:x} fault0=0x{=u8:x} fault1=0x{=u8:x}",
            self.charger.status,
            self.charger.actual_enabled,
            input_present,
            vbat_present,
            self.therm_kill_latched,
            self.charger.vbus_mv,
            self.charger.ibus_ma,
            self.charger.vbat_mv,
            status0,
            status1,
            status2,
            status3,
            status4,
            fault0,
            fault1
        );

        let _ = now;
    }

    fn mark_charger_comm_failed(&mut self, stage: &'static str, err: esp_hal::i2c::master::Error) {
        self.chg_ce.set_high();
        self.charger.actual_enabled = false;
        self.charger.comm_state = SelfCheckCommState::Err;
        self.charger.input_present = None;
        self.charger.vbat_present = None;
        self.charger.vbus_mv = None;
        self.charger.ibus_ma = None;
        self.charger.vbat_mv = None;
        self.charger.status = "ERR";
        self.charger.fault = Some("COMM");
        defmt::error!(
            "tps-test: charger err stage={} err={}",
            stage,
            i2c_error_kind(err)
        );
    }

    fn snapshot(&self) -> TpsTestUiSnapshot {
        TpsTestUiSnapshot {
            build_profile: self.build_profile,
            build_id: self.build_id,
            vout_profile: TEST_VOUT_PROFILE,
            ilim_ma: TEST_ILIMIT_MA,
            charger: self.charger.snapshot(),
            out_a: self.out_a.snapshot(),
            out_b: self.out_b.snapshot(),
            footer_notice: Some("FIXED PROFILE / NO TOUCH CONTROLS"),
            footer_alert: self.footer_alert(),
        }
    }

    fn footer_alert(&self) -> Option<&'static str> {
        if self.therm_kill_latched {
            Some("THERM KILL LATCHED")
        } else if self.charger.comm_state == SelfCheckCommState::Err {
            Some("CHARGER COMM ERROR")
        } else if self.out_a.terminal_fault.is_some() || self.out_b.terminal_fault.is_some() {
            Some("TPS FAULT LATCHED")
        } else if self.out_a.retry_reason.is_some() || self.out_b.retry_reason.is_some() {
            Some("TPS RETRY PENDING")
        } else if !self.ina_ready {
            Some("INA OFFLINE")
        } else {
            None
        }
    }
}

fn step_output(
    i2c: &mut I2c<'static, Blocking>,
    now: Instant,
    ina_ready: bool,
    therm_kill_latched: bool,
    target_vout_mv: u16,
    ilimit_ma: u16,
    ch: OutputChannel,
    state: &mut OutputRuntimeState,
) {
    if therm_kill_latched && state.terminal_fault.is_none() {
        state.terminal_fault = Some("THERM");
    }

    if let Some(fault) = state.terminal_fault {
        let _ = force_disable_output(i2c, ch);
        state.applied = false;
        state.retry_at = None;
        state.retry_reason = None;
        state.actual_enabled = Some(false);
        state.comm_state = match fault {
            "THERM" => SelfCheckCommState::Warn,
            _ => SelfCheckCommState::Warn,
        };
        refresh_output_aux(i2c, ina_ready, ch, state);
        return;
    }

    let retry_due = state
        .retry_at
        .map(|deadline| now >= deadline)
        .unwrap_or(true);
    if !state.applied && retry_due {
        match configure_output(i2c, ch, state.requested_enabled, target_vout_mv, ilimit_ma) {
            Ok(()) => {
                state.applied = true;
                state.retry_at = None;
                state.retry_reason = None;
                state.comm_state = SelfCheckCommState::Ok;
            }
            Err(err) => {
                let _ = force_disable_output(i2c, ch);
                state.applied = false;
                state.actual_enabled = Some(false);
                state.vset_mv = Some(target_vout_mv);
                state.status_bits = None;
                refresh_output_aux(i2c, ina_ready, ch, state);
                if err.retryable {
                    state.retry_reason = Some(err.kind);
                    state.retry_at = Some(now + RETRY_BACKOFF);
                    state.comm_state = SelfCheckCommState::Err;
                } else {
                    state.terminal_fault = Some("CFG");
                    state.retry_reason = None;
                    state.retry_at = None;
                    state.comm_state = SelfCheckCommState::Warn;
                }
                defmt::error!(
                    "tps-test: tps configure err ch={} stage={} kind={} retryable={=bool}",
                    ch.name(),
                    err.stage.as_str(),
                    err.kind,
                    err.retryable
                );
                return;
            }
        }
    }

    if !state.applied {
        refresh_output_aux(i2c, ina_ready, ch, state);
        return;
    }

    match read_telemetry_snapshot(i2c, ch, ina_ready) {
        Ok(telemetry) => {
            state.actual_enabled = telemetry.output_enabled;
            state.vset_mv = telemetry.vset_mv;
            state.vbus_mv = telemetry.vbus_mv;
            state.iout_ma = telemetry.current_ma;
            state.temp_c_x16 = telemetry.temp_c_x16;
            state.status_bits = telemetry.status;
            state.comm_state = SelfCheckCommState::Ok;
            if telemetry.scp || telemetry.ocp || telemetry.ovp {
                state.terminal_fault = Some(decode_tps_fault(
                    telemetry.scp,
                    telemetry.ocp,
                    telemetry.ovp,
                ));
                state.applied = false;
                state.actual_enabled = Some(false);
                state.comm_state = SelfCheckCommState::Warn;
                let _ = force_disable_output(i2c, ch);
                defmt::warn!(
                    "tps-test: tps fault ch={} fault={} status={=?}",
                    ch.name(),
                    state.terminal_fault.unwrap_or("FAULT"),
                    state.status_bits
                );
            }
        }
        Err(kind) => {
            let _ = force_disable_output(i2c, ch);
            state.applied = false;
            state.actual_enabled = Some(false);
            state.retry_reason = Some(kind);
            state.retry_at = Some(now + RETRY_BACKOFF);
            state.comm_state = SelfCheckCommState::Err;
            state.status_bits = None;
            refresh_output_aux(i2c, ina_ready, ch, state);
            defmt::error!("tps-test: tps telemetry err ch={} kind={}", ch.name(), kind);
        }
    }
}

fn refresh_output_aux(
    i2c: &mut I2c<'static, Blocking>,
    ina_ready: bool,
    ch: OutputChannel,
    state: &mut OutputRuntimeState,
) {
    if ina_ready {
        state.vbus_mv = ina3221::read_bus_mv(i2c, ch.ina_ch())
            .ok()
            .and_then(|mv| u16::try_from(mv).ok());
        state.iout_ma = ina3221::read_shunt_uv(i2c, ch.ina_ch())
            .ok()
            .map(|shunt_uv| ina3221::shunt_uv_to_current_ma(shunt_uv, 10));
    }
    state.temp_c_x16 = tmp112::read_temp_c_x16(i2c, ch.tmp_addr()).ok();
}

fn decode_tps_fault(scp: bool, ocp: bool, ovp: bool) -> &'static str {
    if scp && ocp {
        "SCP/OCP"
    } else if scp {
        "SCP"
    } else if ocp {
        "OCP"
    } else if ovp {
        "OVP"
    } else {
        "FAULT"
    }
}

fn charger_status_text(
    charger_fault: bool,
    input_present: bool,
    allow_charge: bool,
    status1: u8,
) -> &'static str {
    if charger_fault {
        "FAULT"
    } else if !input_present {
        "NOAC"
    } else if !TEST_CHARGER_ENABLE {
        "LOCK"
    } else if !allow_charge {
        "WAIT"
    } else {
        match bq25792::status1::chg_stat(status1) {
            1 | 2 | 3 | 4 | 6 => "CHG",
            7 => "DONE",
            _ => "READY",
        }
    }
}

fn decode_charger_fault(
    charger_fault: bool,
    input_present: bool,
    ts_cold: bool,
    ts_hot: bool,
    fault0: u8,
    fault1: u8,
) -> Option<&'static str> {
    if ts_hot {
        Some("TS_HOT")
    } else if ts_cold {
        Some("TS_COLD")
    } else if fault0 != 0 || fault1 != 0 {
        Some("FAULT")
    } else if !input_present && TEST_CHARGER_ENABLE {
        Some("NO INPUT")
    } else {
        let _ = charger_fault;
        None
    }
}
