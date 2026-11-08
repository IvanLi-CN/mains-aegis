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
pub fn apply_profile<T: Transport>(
    transport: &mut T,
) -> Result<crate::charge_profile::ApplyReport<FIELD_COUNT>, ApplyError<T::Error>> {
    crate::charge_profile::apply_definition(transport, CONTAINING_BLOCK_ADDRESS, FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;
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
