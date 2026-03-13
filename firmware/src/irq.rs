use core::sync::atomic::{AtomicU32, Ordering};

use esp_hal::peripherals::GPIO;
use esp_hal::{handler, ram};

pub const GPIO_I2C1_INT: u8 = 33;
pub const GPIO_I2C2_INT: u8 = 7;
pub const GPIO_CHG_INT: u8 = 17;
pub const GPIO_FAN_TACH: u8 = 34;
pub const GPIO_INA_PV: u8 = 37;
pub const GPIO_INA_CRITICAL: u8 = 38;
pub const GPIO_INA_WARNING: u8 = 39;
pub const GPIO_BMS_BTP_INT_H: u8 = 21;
pub const GPIO_THERM_KILL_N: u8 = 40;

static IRQ_I2C1_INT: AtomicU32 = AtomicU32::new(0);
static IRQ_I2C2_INT: AtomicU32 = AtomicU32::new(0);
static IRQ_CHG_INT: AtomicU32 = AtomicU32::new(0);
static IRQ_FAN_TACH: AtomicU32 = AtomicU32::new(0);
static IRQ_INA_PV: AtomicU32 = AtomicU32::new(0);
static IRQ_INA_CRITICAL: AtomicU32 = AtomicU32::new(0);
static IRQ_INA_WARNING: AtomicU32 = AtomicU32::new(0);
static IRQ_BMS_BTP_INT_H: AtomicU32 = AtomicU32::new(0);
static IRQ_THERM_KILL_N: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Default)]
pub struct IrqSnapshot {
    pub i2c1_int: u32,
    pub i2c2_int: u32,
    pub chg_int: u32,
    pub fan_tach: u32,
    pub ina_pv: u32,
    pub ina_critical: u32,
    pub ina_warning: u32,
    pub bms_btp_int_h: u32,
    pub therm_kill_n: u32,
}

impl IrqSnapshot {
    pub fn any(self) -> bool {
        self.i2c1_int != 0
            || self.i2c2_int != 0
            || self.chg_int != 0
            || self.fan_tach != 0
            || self.ina_pv != 0
            || self.ina_critical != 0
            || self.ina_warning != 0
            || self.bms_btp_int_h != 0
            || self.therm_kill_n != 0
    }
}

pub struct IrqTracker {
    last: IrqSnapshot,
}

impl IrqTracker {
    pub const fn new() -> Self {
        Self {
            last: IrqSnapshot {
                i2c1_int: 0,
                i2c2_int: 0,
                chg_int: 0,
                fan_tach: 0,
                ina_pv: 0,
                ina_critical: 0,
                ina_warning: 0,
                bms_btp_int_h: 0,
                therm_kill_n: 0,
            },
        }
    }

    pub fn take_delta(&mut self) -> IrqSnapshot {
        let now = snapshot();
        let delta = IrqSnapshot {
            i2c1_int: now.i2c1_int.wrapping_sub(self.last.i2c1_int),
            i2c2_int: now.i2c2_int.wrapping_sub(self.last.i2c2_int),
            chg_int: now.chg_int.wrapping_sub(self.last.chg_int),
            fan_tach: now.fan_tach.wrapping_sub(self.last.fan_tach),
            ina_pv: now.ina_pv.wrapping_sub(self.last.ina_pv),
            ina_critical: now.ina_critical.wrapping_sub(self.last.ina_critical),
            ina_warning: now.ina_warning.wrapping_sub(self.last.ina_warning),
            bms_btp_int_h: now.bms_btp_int_h.wrapping_sub(self.last.bms_btp_int_h),
            therm_kill_n: now.therm_kill_n.wrapping_sub(self.last.therm_kill_n),
        };
        self.last = now;
        delta
    }
}

pub fn snapshot() -> IrqSnapshot {
    IrqSnapshot {
        i2c1_int: IRQ_I2C1_INT.load(Ordering::Relaxed),
        i2c2_int: IRQ_I2C2_INT.load(Ordering::Relaxed),
        chg_int: IRQ_CHG_INT.load(Ordering::Relaxed),
        fan_tach: IRQ_FAN_TACH.load(Ordering::Relaxed),
        ina_pv: IRQ_INA_PV.load(Ordering::Relaxed),
        ina_critical: IRQ_INA_CRITICAL.load(Ordering::Relaxed),
        ina_warning: IRQ_INA_WARNING.load(Ordering::Relaxed),
        bms_btp_int_h: IRQ_BMS_BTP_INT_H.load(Ordering::Relaxed),
        therm_kill_n: IRQ_THERM_KILL_N.load(Ordering::Relaxed),
    }
}

#[handler(priority = esp_hal::interrupt::Priority::Priority1)]
#[ram]
pub(crate) fn gpio_isr() {
    let regs = GPIO::regs();

    // ESP32-S3: GPIO interrupt status is split into 0..31 and 32..48 banks.
    // Read the CPU interrupt status snapshots (the HAL also uses these internally).
    let pending0 = regs.pcpu_int().read().bits();
    let pending1 = regs.pcpu_int1().read().bits();

    let mut clear0 = 0u32;
    let mut clear1 = 0u32;

    // Bank0 pins (0..31)
    if (pending0 & (1 << GPIO_I2C2_INT)) != 0 {
        IRQ_I2C2_INT.fetch_add(1, Ordering::Relaxed);
        clear0 |= 1 << GPIO_I2C2_INT;
    }
    if (pending0 & (1 << GPIO_CHG_INT)) != 0 {
        IRQ_CHG_INT.fetch_add(1, Ordering::Relaxed);
        clear0 |= 1 << GPIO_CHG_INT;
    }
    if (pending0 & (1 << GPIO_BMS_BTP_INT_H)) != 0 {
        IRQ_BMS_BTP_INT_H.fetch_add(1, Ordering::Relaxed);
        clear0 |= 1 << GPIO_BMS_BTP_INT_H;
    }

    // Bank1 pins (32..)
    let i2c1_int_mask = 1u32 << (GPIO_I2C1_INT - 32);
    if (pending1 & i2c1_int_mask) != 0 {
        IRQ_I2C1_INT.fetch_add(1, Ordering::Relaxed);
        clear1 |= i2c1_int_mask;
    }

    let fan_tach_mask = 1u32 << (GPIO_FAN_TACH - 32);
    if (pending1 & fan_tach_mask) != 0 {
        IRQ_FAN_TACH.fetch_add(1, Ordering::Relaxed);
        clear1 |= fan_tach_mask;
    }

    let ina_pv_mask = 1u32 << (GPIO_INA_PV - 32);
    if (pending1 & ina_pv_mask) != 0 {
        IRQ_INA_PV.fetch_add(1, Ordering::Relaxed);
        clear1 |= ina_pv_mask;
    }

    let ina_critical_mask = 1u32 << (GPIO_INA_CRITICAL - 32);
    if (pending1 & ina_critical_mask) != 0 {
        IRQ_INA_CRITICAL.fetch_add(1, Ordering::Relaxed);
        clear1 |= ina_critical_mask;
    }

    let ina_warning_mask = 1u32 << (GPIO_INA_WARNING - 32);
    if (pending1 & ina_warning_mask) != 0 {
        IRQ_INA_WARNING.fetch_add(1, Ordering::Relaxed);
        clear1 |= ina_warning_mask;
    }

    let therm_kill_mask = 1u32 << (GPIO_THERM_KILL_N - 32);
    if (pending1 & therm_kill_mask) != 0 {
        IRQ_THERM_KILL_N.fetch_add(1, Ordering::Relaxed);
        clear1 |= therm_kill_mask;
    }

    if clear0 != 0 {
        regs.status_w1tc().write(|w| unsafe { w.bits(clear0) });
    }
    if clear1 != 0 {
        regs.status1_w1tc().write(|w| unsafe { w.bits(clear1) });
    }
}
