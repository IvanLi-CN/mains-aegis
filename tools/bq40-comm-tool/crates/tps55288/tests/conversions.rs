use tps55288::registers::{
    code_to_ilim_ma, code_to_vout_mv, ilim_ma_to_code, vout_mv_to_code, ILIM_MAX_MA, VOUT_MAX_MV,
    VOUT_MIN_MV,
};

#[test]
fn vout_roundtrip_mid_range() {
    let mv = 12_000u16;
    let code = vout_mv_to_code(mv);
    let mv_back = code_to_vout_mv(code);
    assert!((VOUT_MIN_MV..=VOUT_MAX_MV).contains(&mv_back));
    assert!((mv_back as i32 - mv as i32).abs() <= 20);
}

#[test]
fn vout_clamps_low_high() {
    let code_low = vout_mv_to_code(100);
    assert_eq!(code_to_vout_mv(code_low), VOUT_MIN_MV);

    let code_high = vout_mv_to_code(30_000);
    assert_eq!(code_to_vout_mv(code_high), VOUT_MAX_MV);
}

#[test]
fn ilim_roundtrip_mid_range() {
    let ma = 3_000u16;
    let code = ilim_ma_to_code(ma);
    let ma_back = code_to_ilim_ma(code);
    assert!(ma_back <= ILIM_MAX_MA);
    assert!((ma_back as i32 - ma as i32).abs() <= 50);
}

#[test]
fn ilim_clamps_high() {
    let code = ilim_ma_to_code(10_000);
    assert_eq!(code_to_ilim_ma(code), ILIM_MAX_MA);
}
