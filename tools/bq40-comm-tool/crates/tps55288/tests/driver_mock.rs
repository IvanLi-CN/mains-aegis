#![cfg(not(feature = "async"))]

use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTrans};
use tps55288::data_types::OperatingStatus;
use tps55288::driver::Tps55288;

#[test]
fn set_vout_writes_ref_registers() {
    // VOUT = 5000 mV -> code (5000-800)/20 = 210 => 0x00D2 (LE: D2 00)
    let expectations = [I2cTrans::write(0x74, vec![0x00, 0xD2, 0x00])];
    let mock = I2cMock::new(&expectations);
    let mut driver = Tps55288::new(mock);
    driver.set_vout_mv(5_000).unwrap();
    driver.free().done();
}

#[test]
fn read_status_decodes_mode_and_faults() {
    // STATUS = 0b1010_0001 -> SCP=1, OCP=0, OVP=1, mode=01 (buck)
    let expectations = [I2cTrans::write_read(0x74, vec![0x07], vec![0b1010_0001])];
    let mock = I2cMock::new(&expectations);
    let mut driver = Tps55288::new(mock);
    let (mode, faults) = driver.read_status().unwrap();
    assert_eq!(mode, OperatingStatus::Buck);
    assert!(faults.short_circuit);
    assert!(!faults.over_current);
    assert!(faults.over_voltage);
    driver.free().done();
}
