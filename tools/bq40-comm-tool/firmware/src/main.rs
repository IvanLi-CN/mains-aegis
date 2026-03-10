#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

mod irq;
mod output;

use esp_backtrace as _;
use esp_firmware::bq40z50;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{DriveMode, Event, Flex, Input, InputConfig, Io, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout};
use esp_hal::ledc::channel::{self, ChannelIFace};
use esp_hal::ledc::timer::{self, TimerIFace};
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed};
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::timer::{systimer::SystemTimer, timg::TimerGroup};
use esp_hal::{main, Blocking};
use esp_println as _;

// Bring-up default profile.
const DEFAULT_ENABLED_OUTPUTS: output::EnabledOutputs = output::EnabledOutputs::Both;

const UPS_VARIANT: &str = if cfg!(feature = "ups-19v") {
    "19v"
} else {
    "12v"
};

// UPS OUT target setpoints.
//
// - 12V variant: margin slightly low to tolerate wiring / load drop.
// - 19V variant: same rationale.
const DEFAULT_VOUT_MV: u16 = if cfg!(feature = "ups-19v") {
    18_240
} else {
    11_240
};

const DEFAULT_ILIMIT_MA: u16 = 3_250;
const TELEMETRY_PERIOD: Duration = Duration::from_millis(500);
const RETRY_BACKOFF: Duration = Duration::from_secs(5);
const FAULT_LOG_MIN_INTERVAL: Duration = Duration::from_millis(200);
const TELEMETRY_INCLUDE_VIN_CH3: bool = false;

// Debug bring-up switches (temporary test knobs).
//
// Development-only override: force a minimal-charge wake path so the gauge can power up.
// Keep disabled by default; normal charging policy remains runtime-driven.
const FORCE_MIN_CHARGE: bool = cfg!(feature = "force-min-charge");
const BMS_DIAG_ISOLATION: bool = true;
const BMS_STRICT_VALIDATION: bool = true;
const BMS_STAGED_PROBE: bool = true;
const BMS_MAC_PROBE_ONLY: bool = cfg!(feature = "bms-mac-probe-only");
// Diagnostic-only build knob: keep MAC-only probing enabled for the whole monitor session.
const BMS_MAC_PROBE_BOOT_WINDOW_SECS: u64 = if cfg!(feature = "bms-mac-probe-only") {
    60 * 60
} else {
    0
};
const BMS_ROM_RECOVER: bool = !cfg!(feature = "bms-rom-recover-disable");
const BMS_PRETOUCH_ENABLED: bool = FORCE_MIN_CHARGE || BMS_ROM_RECOVER;
// Keep the whole diagnostic tool on the slowest known-good SMBus rate to maximize margin during
// recovery and wake attempts.
const I2C1_FREQ_KHZ: u32 = 25;
const I2C1_BUS_CLEAR_PULSES: u8 = 18;
const I2C1_BUS_CLEAR_HALF_PERIOD: Duration = Duration::from_micros(20);
const I2C1_BUS_TIMEOUT_LOW: Duration = Duration::from_millis(40);
const I2C1_BITBANG_HALF_PERIOD: Duration = Duration::from_micros(100);
const BMS_ADDRESS_MODE: bq40z50::BmsAddressMode = if cfg!(feature = "bms-dual-probe-diag") {
    bq40z50::BmsAddressMode::DualProbeDiag
} else {
    bq40z50::BmsAddressMode::Canonical0x0B
};
const SKIP_I2C2_PROBE: bool = false;

const FW_BUILD_PROFILE: &str = env!("FW_BUILD_PROFILE");
const FW_GIT_SHA: &str = env!("FW_GIT_SHA");

// External SYNC for TPS55288 DITH/SYNC pins (SYNCA=0°, SYNCB=180°).
// RFSW on board is 43kΩ (U17/U18 pin 8), so nominal fSW ≈ 20MHz / 43kΩ ≈ 465kHz.
// External clock must be within ±30% of the configured fSW.
// Debug: disable external SYNC to check if INA3221 shunt readings are polluted by coupling.
const TPS_SYNC_ENABLE: bool = false;
const TPS_SYNC_FREQ_KHZ: u32 = 465;
const TPS_SYNC_DUTY_PCT: u8 = 50;
const TPS_SYNC_PHASE_TICKS: u16 = 64; // 180° at Duty7Bit => 128 ticks/period.

// Do not assert THERM_KILL_N during normal bring-up.
const FORCE_THERM_KILL_N_ASSERTED: bool = false;

// TMP112A alert settings (Plan v5hze).
const TMP112_OUT_A_ADDR: u8 = 0x48;
const TMP112_OUT_B_ADDR: u8 = 0x49;
const TMP112_THIGH_C_X16: i16 = 50 * 16;
const TMP112_TLOW_C_X16: i16 = 40 * 16;

fn spin_delay(wait: Duration) {
    let start = Instant::now();
    while Instant::now() - start < wait {
        core::hint::spin_loop();
    }
}

fn prepare_bitbang_pin(pin: &mut Flex<'_>) {
    let input_cfg = InputConfig::default().with_pull(Pull::Up);
    let output_cfg = OutputConfig::default()
        .with_drive_mode(DriveMode::OpenDrain)
        .with_pull(Pull::Up);
    pin.apply_input_config(&input_cfg);
    pin.set_input_enable(true);
    pin.apply_output_config(&output_cfg);
    pin.set_output_enable(true);
    pin.set_high();
}

fn release_bitbang_pin(pin: &mut Flex<'_>) {
    pin.set_high();
    pin.set_output_enable(false);
}

fn bitbang_release(pin: &mut Flex<'_>) {
    pin.set_high();
}

fn bitbang_pull_low(pin: &mut Flex<'_>) {
    pin.set_low();
}

fn bitbang_start(sda: &mut Flex<'_>, scl: &mut Flex<'_>) {
    bitbang_release(sda);
    bitbang_release(scl);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    bitbang_pull_low(sda);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    bitbang_pull_low(scl);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
}

fn bitbang_stop(sda: &mut Flex<'_>, scl: &mut Flex<'_>) {
    bitbang_pull_low(sda);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    bitbang_release(scl);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    bitbang_release(sda);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
}

fn bitbang_write_byte(sda: &mut Flex<'_>, scl: &mut Flex<'_>, byte: u8) -> bool {
    for shift in (0..8).rev() {
        if ((byte >> shift) & 1) != 0 {
            bitbang_release(sda);
        } else {
            bitbang_pull_low(sda);
        }
        spin_delay(I2C1_BITBANG_HALF_PERIOD);
        bitbang_release(scl);
        spin_delay(I2C1_BITBANG_HALF_PERIOD);
        bitbang_pull_low(scl);
    }

    bitbang_release(sda);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    bitbang_release(scl);
    spin_delay(I2C1_BITBANG_HALF_PERIOD / 2);
    let ack = sda.is_low();
    spin_delay(I2C1_BITBANG_HALF_PERIOD / 2);
    bitbang_pull_low(scl);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    ack
}

fn bitbang_touch_bq(sda: &mut Flex<'_>, scl: &mut Flex<'_>, addr: u8, cmd: u8) {
    prepare_bitbang_pin(sda);
    prepare_bitbang_pin(scl);
    bitbang_start(sda, scl);
    let addr_ack = bitbang_write_byte(sda, scl, addr << 1);
    let cmd_ack = if addr_ack {
        bitbang_write_byte(sda, scl, cmd)
    } else {
        false
    };
    bitbang_stop(sda, scl);
    release_bitbang_pin(sda);
    release_bitbang_pin(scl);
    defmt::info!(
        "i2c_bitbang_touch: addr=0x{=u8:x} cmd=0x{=u8:x} addr_ack={=bool} cmd_ack={=bool}",
        addr,
        cmd,
        addr_ack,
        cmd_ack
    );
}

fn clear_i2c_bus(sda: &mut Flex<'_>, scl: &mut Flex<'_>, bus: &'static str) {
    let input_cfg = InputConfig::default().with_pull(Pull::Up);
    let output_cfg = OutputConfig::default()
        .with_drive_mode(DriveMode::OpenDrain)
        .with_pull(Pull::Up);

    sda.apply_input_config(&input_cfg);
    sda.set_input_enable(true);
    sda.set_output_enable(false);
    scl.apply_input_config(&input_cfg);
    scl.set_input_enable(true);
    scl.set_output_enable(false);

    let sda_high_before = sda.is_high();
    let scl_high_before = scl.is_high();

    sda.apply_output_config(&output_cfg);
    scl.apply_output_config(&output_cfg);
    sda.set_high();
    scl.set_high();
    sda.set_output_enable(true);
    scl.set_output_enable(true);
    spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);

    // SMBus devices can recover an incomplete transaction after SCL stays low long enough for
    // the bus-timeout logic to fire. Keep SDA released while we issue this timeout pulse.
    scl.set_low();
    spin_delay(I2C1_BUS_TIMEOUT_LOW);
    scl.set_high();
    spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);

    for _ in 0..I2C1_BUS_CLEAR_PULSES {
        scl.set_low();
        spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);
        scl.set_high();
        spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);
    }

    sda.set_low();
    spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);
    scl.set_high();
    spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);
    sda.set_high();
    spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);

    sda.set_output_enable(false);
    scl.set_output_enable(false);

    let sda_high_after = sda.is_high();
    let scl_high_after = scl.is_high();
    defmt::info!(
        "i2c_bus_clear: bus={} pulses={=u8} timeout_low_ms={=u64} sda_before={=bool} scl_before={=bool} sda_after={=bool} scl_after={=bool}",
        bus,
        I2C1_BUS_CLEAR_PULSES,
        I2C1_BUS_TIMEOUT_LOW.as_millis() as u64,
        sda_high_before,
        scl_high_before,
        sda_high_after,
        scl_high_after
    );
    if !sda_high_after || !scl_high_after {
        defmt::warn!(
            "i2c_bus_clear: bus={} idle_not_high sda_after={=bool} scl_after={=bool}",
            bus,
            sda_high_after,
            scl_high_after
        );
    }
}

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_160MHz);
    let peripherals = esp_hal::init(config);

    // GPIO interrupt aggregator (see `docs/i2c-address-map.md`).
    let mut _io = Io::new(peripherals.IO_MUX);
    _io.set_interrupt_handler(irq::gpio_isr);

    // TPS55288 external sync (SYNCA/SYNCB -> DITH/SYNC).
    // Keep these variables alive for the whole program so PWM keeps running.
    let mut _tps_sync_ledc = Ledc::new(peripherals.LEDC);
    _tps_sync_ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let mut _tps_sync_timer0 = _tps_sync_ledc.timer::<LowSpeed>(timer::Number::Timer0);
    let mut _tps_sync_a = _tps_sync_ledc.channel(channel::Number::Channel0, peripherals.GPIO41);
    let mut _tps_sync_b = _tps_sync_ledc.channel(channel::Number::Channel1, peripherals.GPIO42);

    let mut tps_sync_ok = true;
    if TPS_SYNC_ENABLE {
        match _tps_sync_timer0.configure(timer::config::Config {
            duty: timer::config::Duty::Duty7Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(TPS_SYNC_FREQ_KHZ),
        }) {
            Ok(()) => {
                let ok_a = _tps_sync_a.configure(channel::config::Config {
                    timer: &_tps_sync_timer0,
                    duty_pct: TPS_SYNC_DUTY_PCT,
                    drive_mode: DriveMode::PushPull,
                });
                let ok_b = _tps_sync_b.configure(channel::config::Config {
                    timer: &_tps_sync_timer0,
                    duty_pct: TPS_SYNC_DUTY_PCT,
                    drive_mode: DriveMode::PushPull,
                });

                match (ok_a, ok_b) {
                    (Ok(()), Ok(())) => {
                        // Apply 180° phase shift to SYNCB via hpoint.
                        let ledc_regs = esp_hal::peripherals::LEDC::regs();
                        ledc_regs
                            .ch(1)
                            .hpoint()
                            .write(|w| unsafe { w.hpoint().bits(TPS_SYNC_PHASE_TICKS) });

                        defmt::info!(
                            "power: tps_sync ok freq_khz={} duty_pct={} phase_ticks={=u16}",
                            TPS_SYNC_FREQ_KHZ,
                            TPS_SYNC_DUTY_PCT,
                            TPS_SYNC_PHASE_TICKS
                        );
                    }
                    (a, b) => {
                        tps_sync_ok = false;
                        defmt::error!("power: tps_sync err ch0={=?} ch1={=?}", a, b);
                    }
                }
            }
            Err(e) => {
                tps_sync_ok = false;
                defmt::error!("power: tps_sync timer err={=?}", e);
            }
        }
    } else {
        tps_sync_ok = false;
        defmt::info!("power: tps_sync disabled (pins reserved)");
    }

    // Ensure the system timer is enabled before calling `Instant::now()`.
    let _systimer = SystemTimer::new(peripherals.SYSTIMER);

    // Disable watchdog timers for the simplest possible bring-up loop.
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut wdt0 = timg0.wdt;
    wdt0.disable();

    let timg1 = TimerGroup::new(peripherals.TIMG1);
    let mut wdt1 = timg1.wdt;
    wdt1.disable();

    // Human-readable marker (plain serial) to help bring-up when defmt decoding isn't available yet.
    esp_println::println!("esp: boot (serial)");
    defmt::info!("esp: boot");
    defmt::info!(
        "fw: pkg_version={} git_sha={} profile={}",
        env!("CARGO_PKG_VERSION"),
        FW_GIT_SHA,
        FW_BUILD_PROFILE
    );
    defmt::info!(
        "fw: ups_variant={} default_vout_mv={=u16} default_ilimit_ma={=u16}",
        UPS_VARIANT,
        DEFAULT_VOUT_MV,
        DEFAULT_ILIMIT_MA
    );
    defmt::info!(
        "fw: debug force_min_charge={=bool} bms_diag_isolation={=bool} bms_strict_validation={=bool} bms_staged_probe={=bool} bms_mac_probe_only={=bool} bms_mac_window_s={=u64} bms_rom_recover={=bool} bms_addr_mode={} skip_i2c2_probe={=bool}",
        FORCE_MIN_CHARGE,
        BMS_DIAG_ISOLATION,
        BMS_STRICT_VALIDATION,
        BMS_STAGED_PROBE,
        BMS_MAC_PROBE_ONLY,
        BMS_MAC_PROBE_BOOT_WINDOW_SECS,
        BMS_ROM_RECOVER,
        BMS_ADDRESS_MODE.as_str(),
        SKIP_I2C2_PROBE
    );
    defmt::info!(
        "fw: bms_addr_semantics addr7=0x{=u8:x} addr8_w=0x{=u8:x} addr8_r=0x{=u8:x}",
        bq40z50::I2C_ADDRESS_PRIMARY,
        bq40z50::I2C_ADDRESS_PRIMARY << 1,
        (bq40z50::I2C_ADDRESS_PRIMARY << 1) | 1
    );
    if matches!(BMS_ADDRESS_MODE, bq40z50::BmsAddressMode::DualProbeDiag) {
        defmt::warn!("fw: bms dual-probe diagnostic mode enabled");
    }
    defmt::info!("fw: i2c1_khz={=u32}", I2C1_FREQ_KHZ);

    let mut i2c1_sda = Flex::new(peripherals.GPIO48);
    let mut i2c1_scl = Flex::new(peripherals.GPIO47);
    if BMS_PRETOUCH_ENABLED {
        clear_i2c_bus(&mut i2c1_sda, &mut i2c1_scl, "i2c1");
        bitbang_touch_bq(
            &mut i2c1_sda,
            &mut i2c1_scl,
            bq40z50::I2C_ADDRESS_PRIMARY,
            0x0d,
        );
    }

    let i2c1_config = I2cConfig::default()
        .with_frequency(Rate::from_khz(I2C1_FREQ_KHZ))
        .with_software_timeout(SoftwareTimeout::Transaction(Duration::from_millis(100)));
    let mut i2c: I2c<'static, Blocking> = I2c::new(peripherals.I2C1, i2c1_config)
        .unwrap()
        .with_sda(i2c1_sda.into_peripheral_output())
        .with_scl(i2c1_scl.into_peripheral_output());

    let i2c1_int_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut i2c1_int = Input::new(peripherals.GPIO33, i2c1_int_cfg);
    i2c1_int.clear_interrupt();
    i2c1_int.listen(Event::FallingEdge);

    // I2C2 interrupt/alert line (open-drain, active-low).
    let i2c2_int_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut _i2c2_int = Input::new(peripherals.GPIO7, i2c2_int_cfg);
    _i2c2_int.clear_interrupt();
    _i2c2_int.listen(Event::FallingEdge);

    // INA3221 alerts (open-drain, active-low).
    let ina_alert_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut _ina_pv = Input::new(peripherals.GPIO37, ina_alert_cfg);
    _ina_pv.clear_interrupt();
    _ina_pv.listen(Event::FallingEdge);
    let mut _ina_critical = Input::new(peripherals.GPIO38, ina_alert_cfg);
    _ina_critical.clear_interrupt();
    _ina_critical.listen(Event::FallingEdge);
    let mut _ina_warning = Input::new(peripherals.GPIO39, ina_alert_cfg);
    _ina_warning.clear_interrupt();
    _ina_warning.listen(Event::FallingEdge);

    // BMS interrupt/alert line (active-high on MCU side after an inverter stage).
    let bms_btp_int_cfg = InputConfig::default().with_pull(Pull::None);
    let mut bms_btp_int_h = Input::new(peripherals.GPIO21, bms_btp_int_cfg);
    bms_btp_int_h.clear_interrupt();
    bms_btp_int_h.listen(Event::RisingEdge);

    // BQ25792 charger control pins.
    //
    // CE is active-low; hardware pull-up keeps it disabled during reset. We still
    // drive it HIGH as early as possible and only enable charging after we can
    // validate charger/battery status over I2C.
    let mut chg_ce = Flex::new(peripherals.GPIO15);
    chg_ce.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::OpenDrain)
            .with_pull(Pull::Up),
    );
    chg_ce.set_high();
    chg_ce.set_output_enable(true);

    // ILIM_HIZ "brake" (drives an external NMOS that pulls ILIM_HIZ low).
    // Keep deasserted (LOW) during normal bring-up.
    let mut chg_ilim_hiz_brk = Flex::new(peripherals.GPIO16);
    chg_ilim_hiz_brk.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::PushPull)
            .with_pull(Pull::None),
    );
    chg_ilim_hiz_brk.set_low();
    chg_ilim_hiz_brk.set_output_enable(true);

    // CHG_INT is an open-drain 256us active-low pulse. We poll status registers
    // periodically, but also count pulses via ISR for timely snapshots.
    let chg_int_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut _chg_int = Input::new(peripherals.GPIO17, chg_int_cfg);
    _chg_int.clear_interrupt();
    _chg_int.listen(Event::FallingEdge);

    // Ensure THERM_KILL_N is released. This net can hard-disable both TPS via TPS_EN.
    // Configure as open-drain output, set HIGH (release), and also enable input so we can observe if
    // something external is holding it low.
    let mut therm_kill = Flex::new(peripherals.GPIO40);
    therm_kill.apply_input_config(&InputConfig::default().with_pull(Pull::Up));
    therm_kill.set_input_enable(true);
    let low_before = therm_kill.is_low();
    therm_kill.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::OpenDrain)
            .with_pull(Pull::Up),
    );
    therm_kill.set_high();
    therm_kill.set_output_enable(true);
    if FORCE_THERM_KILL_N_ASSERTED {
        therm_kill.set_low();
    }
    let low_after = therm_kill.is_low();
    therm_kill.clear_interrupt();
    therm_kill.listen(Event::FallingEdge);
    defmt::info!(
        "power: therm_kill_n low_before={=bool} low_after={=bool} forced={=bool}",
        low_before,
        low_after,
        FORCE_THERM_KILL_N_ASSERTED
    );
    if low_after {
        defmt::warn!(
            "power: therm_kill_n asserted; TPS_EN likely forced low (power stage disabled)"
        );
    }

    // Program TMP112A alert thresholds and debounce.
    let tmp112_cfg = esp_firmware::tmp112::AlertConfig {
        t_high_c_x16: TMP112_THIGH_C_X16,
        t_low_c_x16: TMP112_TLOW_C_X16,
        fault_queue: esp_firmware::tmp112::FaultQueue::F4,
        conversion_rate: esp_firmware::tmp112::ConversionRate::Hz1,
    };
    let mut tmp_out_a_ok = false;
    let mut tmp_out_b_ok = false;
    for addr in [TMP112_OUT_A_ADDR, TMP112_OUT_B_ADDR] {
        match esp_firmware::tmp112::program_alert_config(&mut i2c, addr, tmp112_cfg) {
            Ok(rb) => {
                defmt::info!(
                    "power: tmp112 ok addr=0x{=u8:x} cfg=0x{=u16:x} tlow=0x{=u16:x} thigh=0x{=u16:x}",
                    addr,
                    rb.config,
                    rb.tlow,
                    rb.thigh
                );
                if addr == TMP112_OUT_A_ADDR {
                    tmp_out_a_ok = true;
                }
                if addr == TMP112_OUT_B_ADDR {
                    tmp_out_b_ok = true;
                }
            }
            Err(e) => {
                defmt::error!(
                    "power: tmp112 err addr=0x{=u8:x} err={}",
                    addr,
                    output::i2c_error_kind(e)
                );
            }
        }
    }
    if !tmp_out_a_ok && !tmp_out_b_ok {
        defmt::error!(
            "power: tmp112 init failed for both channels; outputs likely disabled (self-test)"
        );
    }

    // Boot self-test: detect online devices and decide which modules are allowed to run.
    if !SKIP_I2C2_PROBE {
        let i2c2_config = I2cConfig::default()
            .with_frequency(Rate::from_khz(400))
            .with_software_timeout(SoftwareTimeout::Transaction(Duration::from_millis(100)));
        let mut i2c2: I2c<'static, Blocking> = I2c::new(peripherals.I2C0, i2c2_config)
            .unwrap()
            .with_sda(peripherals.GPIO8)
            .with_scl(peripherals.GPIO9);
        output::log_i2c2_presence(&mut i2c2);
    } else {
        defmt::info!("self_test: i2c2 scan skipped");
    }

    let self_test = output::boot_self_test(
        &mut i2c,
        DEFAULT_ENABLED_OUTPUTS,
        DEFAULT_VOUT_MV,
        DEFAULT_ILIMIT_MA,
        TELEMETRY_INCLUDE_VIN_CH3,
        tmp_out_a_ok,
        tmp_out_b_ok,
        tps_sync_ok,
        BMS_ADDRESS_MODE,
        BMS_STRICT_VALIDATION,
    );

    let cfg = output::Config {
        enabled_outputs: self_test.enabled_outputs,
        vout_mv: DEFAULT_VOUT_MV,
        ilimit_ma: DEFAULT_ILIMIT_MA,
        telemetry_period: TELEMETRY_PERIOD,
        retry_backoff: RETRY_BACKOFF,
        fault_log_min_interval: FAULT_LOG_MIN_INTERVAL,
        telemetry_include_vin_ch3: TELEMETRY_INCLUDE_VIN_CH3,
        tmp112_tlow_c_x16: TMP112_TLOW_C_X16,
        tmp112_thigh_c_x16: TMP112_THIGH_C_X16,
        charger_enabled: self_test.charger_enabled,
        charge_allowed: true,
        force_min_charge: FORCE_MIN_CHARGE,
        bms_addr: self_test.bms_addr,
        bms_diag_isolation: BMS_DIAG_ISOLATION,
        bms_address_mode: BMS_ADDRESS_MODE,
        bms_strict_validation: BMS_STRICT_VALIDATION,
        bms_staged_probe: BMS_STAGED_PROBE,
        bms_mac_probe_only: BMS_MAC_PROBE_ONLY,
        bms_mac_probe_boot_window: Duration::from_secs(BMS_MAC_PROBE_BOOT_WINDOW_SECS),
        bms_rom_recover: BMS_ROM_RECOVER,
    };

    let mut power = output::PowerManager::new(
        i2c,
        i2c1_int,
        bms_btp_int_h,
        therm_kill,
        chg_ce,
        chg_ilim_hiz_brk,
        cfg,
    );
    defmt::info!(
        "power: enabled_outputs={} target_vout_mv={=u16} target_ilimit_ma={=u16}",
        cfg.enabled_outputs.describe(),
        cfg.vout_mv,
        cfg.ilimit_ma
    );
    power.init_best_effort();

    let mut irq_tracker = irq::IrqTracker::new();

    loop {
        defmt::info!("esp: heartbeat");
        let irq_events = irq_tracker.take_delta();
        power.tick(&irq_events);
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(2_000) {}
    }
}
