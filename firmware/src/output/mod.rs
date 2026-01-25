pub mod tps55288;

use esp_firmware::ina3221;
use esp_hal::gpio::Input;
use esp_hal::time::{Duration, Instant};

use ::tps55288::Error as TpsError;

pub use self::tps55288::OutputChannel;

#[derive(Clone, Copy)]
pub enum TelemetryValue {
    Value(i32),
    Err(&'static str),
}

impl defmt::Format for TelemetryValue {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            TelemetryValue::Value(v) => defmt::write!(fmt, "{}", v),
            TelemetryValue::Err(kind) => defmt::write!(fmt, "err({})", kind),
        }
    }
}

#[derive(Clone, Copy)]
pub enum TelemetryU8 {
    Value(u8),
    Err(&'static str),
}

impl defmt::Format for TelemetryU8 {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            TelemetryU8::Value(v) => defmt::write!(fmt, "0x{=u8:x}", v),
            TelemetryU8::Err(kind) => defmt::write!(fmt, "err({})", kind),
        }
    }
}

#[derive(Clone, Copy)]
pub enum TelemetryBool {
    Value(bool),
    Err(&'static str),
}

impl defmt::Format for TelemetryBool {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            TelemetryBool::Value(v) => defmt::write!(fmt, "{}", v),
            TelemetryBool::Err(kind) => defmt::write!(fmt, "err({})", kind),
        }
    }
}

pub(super) fn i2c_error_kind(err: esp_hal::i2c::master::Error) -> &'static str {
    use esp_hal::i2c::master::Error;
    match err {
        Error::Timeout => "i2c_timeout",
        Error::AcknowledgeCheckFailed(_) => "i2c_nack",
        Error::ArbitrationLost => "i2c_arbitration",
        _ => "i2c",
    }
}

pub(super) fn tps_error_kind(err: TpsError<esp_hal::i2c::master::Error>) -> &'static str {
    match err {
        TpsError::I2c(e) => i2c_error_kind(e),
        TpsError::OutOfRange => "out_of_range",
        TpsError::InvalidConfig => "invalid_config",
    }
}

pub(super) fn ina_error_kind(err: ina3221::Error<esp_hal::i2c::master::Error>) -> &'static str {
    match err {
        ina3221::Error::I2c(e) => i2c_error_kind(e),
        ina3221::Error::OutOfRange => "out_of_range",
        ina3221::Error::InvalidConfig => "invalid_config",
    }
}

pub struct PowerManager<'d, I2C> {
    i2c: I2C,
    i2c1_int: Input<'d>,

    cfg: Config,

    next_telemetry_at: Instant,
    last_fault_log_at: Option<Instant>,

    ina_ready: bool,
    ina_next_retry_at: Option<Instant>,

    tps_a_ready: bool,
    tps_a_next_retry_at: Option<Instant>,
    tps_b_ready: bool,
    tps_b_next_retry_at: Option<Instant>,
}

#[derive(Clone, Copy)]
pub struct Config {
    pub default_enabled: OutputChannel,
    pub vout_mv: u16,
    pub ilimit_ma: u16,
    pub telemetry_period: Duration,
    pub retry_backoff: Duration,
    pub fault_log_min_interval: Duration,
    pub telemetry_include_vin_ch3: bool,
}

impl<'d, I2C> PowerManager<'d, I2C>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    pub fn new(i2c: I2C, i2c1_int: Input<'d>, cfg: Config) -> Self {
        let now = Instant::now();
        Self {
            i2c,
            i2c1_int,
            cfg,

            next_telemetry_at: now,
            last_fault_log_at: None,

            ina_ready: false,
            ina_next_retry_at: Some(now),

            tps_a_ready: false,
            tps_a_next_retry_at: Some(now),
            tps_b_ready: false,
            tps_b_next_retry_at: Some(now),
        }
    }

    pub fn init_best_effort(&mut self) {
        self.try_init_ina();
        self.try_configure_tps(OutputChannel::OutA);
        self.try_configure_tps(OutputChannel::OutB);
    }

    pub fn tick(&mut self) {
        self.maybe_retry();
        self.maybe_handle_fault();
        self.maybe_print_telemetry();
    }

    fn maybe_retry(&mut self) {
        let now = Instant::now();

        if !self.ina_ready {
            if let Some(t) = self.ina_next_retry_at {
                if now >= t {
                    self.ina_next_retry_at = None;
                    self.try_init_ina();
                }
            }
        }

        if !self.tps_a_ready {
            if let Some(t) = self.tps_a_next_retry_at {
                if now >= t {
                    self.tps_a_next_retry_at = None;
                    self.try_configure_tps(OutputChannel::OutA);
                }
            }
        }

        if !self.tps_b_ready {
            if let Some(t) = self.tps_b_next_retry_at {
                if now >= t {
                    self.tps_b_next_retry_at = None;
                    self.try_configure_tps(OutputChannel::OutB);
                }
            }
        }
    }

    fn try_init_ina(&mut self) {
        let cfg = if self.cfg.telemetry_include_vin_ch3 {
            ina3221::CONFIG_VALUE_CH123
        } else {
            ina3221::CONFIG_VALUE_CH12
        };

        match ina3221::init_with_config(&mut self.i2c, cfg) {
            Ok(()) => {
                self.ina_ready = true;
                defmt::info!("power: ina3221 ok (addr=0x40 config=0x{=u16:x})", cfg);
            }
            Err(e) => {
                self.ina_ready = false;
                self.ina_next_retry_at = Some(Instant::now() + self.cfg.retry_backoff);
                defmt::error!("power: ina3221 err={}", ina_error_kind(e));
            }
        }
    }

    fn try_configure_tps(&mut self, ch: OutputChannel) {
        let enabled = ch == self.cfg.default_enabled;
        let addr = ch.addr();

        match tps55288::configure_one(
            &mut self.i2c,
            ch,
            enabled,
            self.cfg.vout_mv,
            self.cfg.ilimit_ma,
        ) {
            Ok(()) => {
                tps55288::log_configured(&mut self.i2c, ch, enabled);
                self.mark_tps_ok(ch);
            }
            Err((stage, e)) => {
                let kind = tps_error_kind(e);
                self.mark_tps_failed(ch, Instant::now() + self.cfg.retry_backoff);
                defmt::error!(
                    "power: tps addr=0x{=u8:x} stage={} err={}",
                    addr,
                    stage.as_str(),
                    kind
                );
                if kind == "i2c_nack" && ch == OutputChannel::OutB {
                    defmt::warn!(
                        "power: tps addr=0x75 nack_hint=maybe_address_changed; power-cycle TPS rails to restore preset address"
                    );
                }
            }
        }
    }

    fn mark_tps_ok(&mut self, ch: OutputChannel) {
        match ch {
            OutputChannel::OutA => self.tps_a_ready = true,
            OutputChannel::OutB => self.tps_b_ready = true,
        }
    }

    fn mark_tps_failed(&mut self, ch: OutputChannel, next: Instant) {
        match ch {
            OutputChannel::OutA => {
                self.tps_a_ready = false;
                self.tps_a_next_retry_at = Some(next);
            }
            OutputChannel::OutB => {
                self.tps_b_ready = false;
                self.tps_b_next_retry_at = Some(next);
            }
        }
    }

    fn maybe_handle_fault(&mut self) {
        let now = Instant::now();
        if self.i2c1_int.is_low() {
            if tps55288::should_log_fault(
                now,
                &mut self.last_fault_log_at,
                self.cfg.fault_log_min_interval,
            ) {
                tps55288::log_fault_status(&mut self.i2c, OutputChannel::OutA, self.ina_ready);
                tps55288::log_fault_status(&mut self.i2c, OutputChannel::OutB, self.ina_ready);
            }
        }
    }

    fn maybe_print_telemetry(&mut self) {
        let now = Instant::now();
        if now < self.next_telemetry_at {
            return;
        }
        self.next_telemetry_at = now + self.cfg.telemetry_period;

        tps55288::print_telemetry_line(&mut self.i2c, OutputChannel::OutA, self.ina_ready);
        tps55288::print_telemetry_line(&mut self.i2c, OutputChannel::OutB, self.ina_ready);

        if self.cfg.telemetry_include_vin_ch3 && self.ina_ready {
            let bus = ina3221::read_bus_mv(&mut self.i2c, ina3221::Channel::Ch3);
            let shunt = ina3221::read_shunt_uv(&mut self.i2c, ina3221::Channel::Ch3);
            let vbus_mv = match bus {
                Ok(v) => TelemetryValue::Value(v),
                Err(e) => TelemetryValue::Err(ina_error_kind(e)),
            };
            let current_ma = match shunt {
                Ok(shunt_uv) => TelemetryValue::Value(ina3221::shunt_uv_to_current_ma(shunt_uv, 7)),
                Err(e) => TelemetryValue::Err(ina_error_kind(e)),
            };
            defmt::info!(
                "telemetry ch=vin addr=0x40 vbus_mv={} current_ma={}",
                vbus_mv,
                current_ma
            );
        } else {
            defmt::info!(
                "telemetry ch=vin addr=0x40 vbus_mv={} current_ma={}",
                TelemetryValue::Err("ina_uninit"),
                TelemetryValue::Err("ina_uninit")
            );
        }
    }
}
