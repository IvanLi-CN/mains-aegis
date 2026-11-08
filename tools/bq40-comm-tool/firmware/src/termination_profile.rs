//! Independent, one-field R2 charge-termination allowlist.
//! The temperature writer's seven-field definition remains unchanged.
pub use crate::charge_profile::{
    build_mb44_field_write_request, build_mb44_read_request, decode_plain_mb44_block, ApplyError,
    BlockReadDecodeError, BlockSnapshot, Field, FieldVerification, Transport,
};
pub const CONTAINING_BLOCK_ADDRESS: u16 = 0x4A60;
pub const CONTAINING_BLOCK_LEN: u8 = 32;
pub const TAPER_MA: u16 = 120;
pub const FIELD_COUNT: usize = 1;
pub const FIELDS: [Field; FIELD_COUNT] = [Field {
    name: "charge_term_taper_current",
    address: 0x4A6D,
    offset: 13,
    raw: TAPER_MA.to_le_bytes(),
}];
/// TRM 14.1.3 uses DDddVVvvBBbbTTZZzzRREE (MSB first), unlike SBS words/DF.
pub fn firmware_is_r2(payload: &[u8]) -> bool {
    payload.len() == 11 && u16::from_be_bytes([payload[0], payload[1]]) == 0x4500 && payload[2] == 2
}

pub fn apply_profile<T: Transport>(
    transport: &mut T,
) -> Result<crate::charge_profile::ApplyReport<FIELD_COUNT>, ApplyError<T::Error>> {
    crate::charge_profile::apply_definition(transport, CONTAINING_BLOCK_ADDRESS, FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn r2_identity_uses_full_big_endian_firmware_response() {
        let live = [0x45, 0, 2, 0x11, 0, 0x34, 0, 4, 0x75, 2, 0];
        assert!(firmware_is_r2(&live));
        assert!(!firmware_is_r2(&live[..4]));
        let mut wrong = live;
        wrong.swap(0, 1);
        assert!(!firmware_is_r2(&wrong));
        wrong = live;
        wrong[2] = 5;
        assert!(!firmware_is_r2(&wrong));
    }

    struct Bus {
        block: [u8; 32],
        writes: usize,
        corrupt: bool,
    }
    impl Transport for Bus {
        type Error = ();
        fn read_block(&mut self, address: u16) -> Result<BlockSnapshot, ()> {
            Ok(BlockSnapshot {
                address,
                declared_len: 32,
                payload_len: 32,
                payload: self.block,
            })
        }
        fn write_field(&mut self, address: u16, raw: [u8; 2]) -> Result<BlockSnapshot, ()> {
            assert_eq!(address, 0x4A6D);
            assert_eq!(raw, [120, 0]);
            self.writes += 1;
            self.block[13..15].copy_from_slice(&raw);
            if self.corrupt {
                self.block[0] ^= 1;
            }
            let mut payload = [0; 32];
            payload[..2].copy_from_slice(&raw);
            Ok(BlockSnapshot {
                address,
                declared_len: 32,
                payload_len: 32,
                payload,
            })
        }
    }
    #[test]
    fn changes_only_taper_and_preserves_every_other_byte() {
        let mut bus = Bus {
            block: [0xa5; 32],
            writes: 0,
            corrupt: false,
        };
        let report = apply_profile(&mut bus).unwrap();
        assert_eq!(bus.writes, 1);
        for i in 0..32 {
            assert_eq!(
                report.final_snapshot.payload[i],
                match i {
                    13 => 120,
                    14 => 0,
                    _ => 0xa5,
                }
            );
        }
        assert_eq!(report.field_verifications[0].decoded, 120);
    }
    #[test]
    fn collateral_write_is_rejected_with_original_snapshot_and_no_rollback() {
        let mut bus = Bus {
            block: [0xa5; 32],
            writes: 0,
            corrupt: true,
        };
        let err = apply_profile(&mut bus).unwrap_err();
        assert_eq!(err.as_str(), "final_raw");
        assert_eq!(err.snapshot.unwrap().payload, [0xa5; 32]);
        assert_eq!(bus.writes, 1);
    }
}
