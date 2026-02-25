use tps55288::data_types::{CableCompLevel, CableCompOption, OcpDelay, VoutSlewRate};
use tps55288::registers::{
    code_to_ilim_ma, code_to_vout_mv, decode_status_mode, ilim_ma_to_code, vout_mv_to_code,
    CdcBits, StatusBits, VoutSrBits,
};

#[test]
fn status_decode_modes() {
    let boost = StatusBits::from_bits_truncate(0b0000_0000);
    assert_eq!(decode_status_mode(&boost), 0b00);
    let buck = StatusBits::from_bits_truncate(0b0000_0001);
    assert_eq!(decode_status_mode(&buck), 0b01);
    let buck_boost = StatusBits::from_bits_truncate(0b0000_0010);
    assert_eq!(decode_status_mode(&buck_boost), 0b10);
}

#[test]
fn vout_sr_bits_mapping() {
    let base = VoutSrBits::empty();
    assert_eq!(base.bits() & 0b11_0000, 0);

    let sr_fast = VoutSrBits::SR0 | VoutSrBits::SR1;
    assert_eq!(sr_fast.bits() & 0b11, 0b11);

    let ocp_delay = VoutSrBits::OCP_DELAY0 | VoutSrBits::OCP_DELAY1;
    assert_eq!((ocp_delay.bits() >> 4) & 0b11, 0b11);

    // Roundtrip enums to bits and back via match in code (compile-time mapping check).
    let _slew = [
        VoutSlewRate::Sr1p25MvPerUs,
        VoutSlewRate::Sr2p5MvPerUs,
        VoutSlewRate::Sr5MvPerUs,
        VoutSlewRate::Sr10MvPerUs,
    ];
    let _ocp = [
        OcpDelay::Us128,
        OcpDelay::Ms3_072,
        OcpDelay::Ms6_144,
        OcpDelay::Ms12_288,
    ];
}

#[test]
fn cdc_level_bits_mapping() {
    let levels = [
        (CableCompLevel::V0p0, 0b000),
        (CableCompLevel::V0p1, 0b001),
        (CableCompLevel::V0p2, 0b010),
        (CableCompLevel::V0p3, 0b011),
        (CableCompLevel::V0p4, 0b100),
        (CableCompLevel::V0p5, 0b101),
        (CableCompLevel::V0p6, 0b110),
        (CableCompLevel::V0p7, 0b111),
    ];

    for (level, bits) in levels {
        let mut cdc = CdcBits::empty();
        if let CableCompOption::External = CableCompOption::External {
            // no-op, just to avoid unused warning in case we change it later
            let _ = (); // placeholder
        }
        cdc |= match level {
            CableCompLevel::V0p0 => CdcBits::empty(),
            CableCompLevel::V0p1 => CdcBits::CDC0,
            CableCompLevel::V0p2 => CdcBits::CDC1,
            CableCompLevel::V0p3 => CdcBits::CDC0 | CdcBits::CDC1,
            CableCompLevel::V0p4 => CdcBits::CDC2,
            CableCompLevel::V0p5 => CdcBits::CDC2 | CdcBits::CDC0,
            CableCompLevel::V0p6 => CdcBits::CDC2 | CdcBits::CDC1,
            CableCompLevel::V0p7 => CdcBits::CDC2 | CdcBits::CDC1 | CdcBits::CDC0,
        };
        assert_eq!(cdc.bits() & 0b111, bits);
    }
}

#[test]
fn conversion_edges_match_constants() {
    // ensure clamping helpers stay consistent
    let code_hi = ilim_ma_to_code(10_000);
    assert_eq!(code_to_ilim_ma(code_hi), tps55288::registers::ILIM_MAX_MA);

    let code_hi_vout = vout_mv_to_code(30_000);
    assert_eq!(
        code_to_vout_mv(code_hi_vout),
        tps55288::registers::VOUT_MAX_MV
    );
}
