//! Two-field R2 capacity model configuration. Never resets learning or calibration.
use crate::charge_profile::{self, ApplyErrorKind, ApplyReport, BlockSnapshot, Field, Transport};

pub const DESIGN_CAPACITY_MAH: u16 = 5000;
pub const TERM_VOLTAGE_MV: u16 = 11200;
pub const BLOCKS: [u16; 2] = [0x48E0, 0x4840];
pub const FIELDS: [Field; 2] = [
    Field {
        name: "design_capacity_mah",
        address: 0x48E5,
        offset: 5,
        raw: DESIGN_CAPACITY_MAH.to_le_bytes(),
    },
    Field {
        name: "term_voltage_mv",
        address: 0x484A,
        offset: 10,
        raw: TERM_VOLTAGE_MV.to_le_bytes(),
    },
];

/// Reject a conflicting cell count or per-cell termination mode; never repair
/// these neighbouring fields implicitly while changing the capacity model.
pub fn prerequisites_ok(
    charger_enabled: bool,
    da_configuration: u16,
    it_configuration: u16,
) -> bool {
    !charger_enabled && da_configuration & 3 == 3 && it_configuration & (1 << 9) == 0
}

/// Project overlay only; never edits the immutable factory image on disk.
pub fn patch_restore_overlay(section1: &mut [u8]) -> bool {
    if section1.len() < 0x08E7 {
        return false;
    }
    for field in FIELDS {
        let offset = (field.address - 0x4000) as usize;
        if offset + 2 > section1.len() {
            return false;
        }
    }
    for field in FIELDS {
        let offset = (field.address - 0x4000) as usize;
        section1[offset..offset + 2].copy_from_slice(&field.raw);
    }
    true
}

#[derive(Debug)]
pub struct Error<E: Copy> {
    pub block: u16,
    pub kind: ApplyErrorKind<E>,
    pub snapshots: [Option<BlockSnapshot>; 2],
}
impl<E: Copy> Error<E> {
    pub fn as_str(&self) -> &'static str {
        charge_profile::ApplyError {
            kind: self.kind,
            snapshot: None,
        }
        .as_str()
    }
}

/// Capture both complete blocks before the first write. Partial failure stops;
/// the pre-write snapshots remain available and no rollback is attempted.
pub fn apply_profile<T: Transport>(
    transport: &mut T,
) -> Result<[ApplyReport<1>; 2], Error<T::Error>> {
    let mut snapshots = [None; 2];
    for (i, address) in BLOCKS.into_iter().enumerate() {
        let snapshot = transport.read_block(address).map_err(|e| Error {
            block: address,
            kind: ApplyErrorKind::SnapshotTransport(e),
            snapshots,
        })?;
        snapshots[i] = Some(snapshot);
        let kind = if snapshot.address != address {
            Some(ApplyErrorKind::SnapshotAddress)
        } else if snapshot.declared_len != 32 || snapshot.payload_len != 32 {
            Some(ApplyErrorKind::SnapshotLength)
        } else {
            None
        };
        if let Some(kind) = kind {
            return Err(Error {
                block: address,
                kind,
                snapshots,
            });
        }
    }
    let mut reports = [None; 2];
    for i in 0..2 {
        reports[i] = Some(
            charge_profile::apply_snapshot(
                transport,
                BLOCKS[i],
                [FIELDS[i]],
                snapshots[i].unwrap(),
            )
            .map_err(|e| Error {
                block: BLOCKS[i],
                kind: e.kind,
                snapshots,
            })?,
        );
    }
    // Re-read both blocks after ALL writes, including the first block. A second
    // field write must not invalidate an earlier successful verification.
    for i in 0..2 {
        let final_snapshot = transport.read_block(BLOCKS[i]).map_err(|e| Error {
            block: BLOCKS[i],
            kind: ApplyErrorKind::FinalTransport(e),
            snapshots,
        })?;
        let report = reports[i].as_mut().unwrap();
        let kind = if final_snapshot.address != BLOCKS[i] {
            Some(ApplyErrorKind::FinalAddress)
        } else if final_snapshot.declared_len != 32 || final_snapshot.payload_len != 32 {
            Some(ApplyErrorKind::FinalLength)
        } else if final_snapshot.payload != report.final_snapshot.payload {
            Some(ApplyErrorKind::FinalRaw)
        } else {
            None
        };
        if let Some(kind) = kind {
            return Err(Error {
                block: BLOCKS[i],
                kind,
                snapshots,
            });
        }
        report.final_snapshot = final_snapshot;
    }
    Ok(reports.map(Option::unwrap))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Copy)]
    enum Fault {
        None,
        PreflightTransport,
        PreflightAddress,
        PreflightLength,
        Write,
        EchoAddress,
        EchoLength,
        EchoRaw,
        Collateral,
        LateCollateral,
        FinalTransport,
        FinalAddress,
        FinalLength,
    }
    struct Bus {
        blocks: [[u8; 32]; 2],
        writes: usize,
        reads: usize,
        fault: Fault,
    }
    impl Bus {
        fn new(fault: Fault) -> Self {
            Self {
                blocks: [[0xa5; 32]; 2],
                writes: 0,
                reads: 0,
                fault,
            }
        }
    }
    impl Transport for Bus {
        type Error = ();
        fn read_block(&mut self, address: u16) -> Result<BlockSnapshot, ()> {
            self.reads += 1;
            let i = BLOCKS.iter().position(|a| *a == address).unwrap();
            if self.reads == 2 && matches!(self.fault, Fault::PreflightTransport) {
                return Err(());
            }
            if self.reads == 5 && matches!(self.fault, Fault::FinalTransport) {
                return Err(());
            }
            let mut s = BlockSnapshot {
                address,
                declared_len: 32,
                payload_len: 32,
                payload: self.blocks[i],
            };
            if self.reads == 2 {
                if matches!(self.fault, Fault::PreflightAddress) {
                    s.address += 1;
                }
                if matches!(self.fault, Fault::PreflightLength) {
                    s.declared_len = 31;
                }
            }
            if self.reads == 5 {
                if matches!(self.fault, Fault::FinalAddress) {
                    s.address += 1;
                }
                if matches!(self.fault, Fault::FinalLength) {
                    s.payload_len = 31;
                }
            }
            Ok(s)
        }
        fn write_field(&mut self, address: u16, raw: [u8; 2]) -> Result<BlockSnapshot, ()> {
            assert!(
                self.reads >= 2,
                "both blocks must be captured before writing"
            );
            let i = FIELDS.iter().position(|f| f.address == address).unwrap();
            assert_eq!(raw, FIELDS[i].raw);
            if i == 1 && matches!(self.fault, Fault::Write) {
                return Err(());
            }
            self.writes += 1;
            let offset = FIELDS[i].offset as usize;
            self.blocks[i][offset..offset + 2].copy_from_slice(&raw);
            if matches!(self.fault, Fault::Collateral) {
                self.blocks[i][0] ^= 1;
            }
            if i == 1 && matches!(self.fault, Fault::LateCollateral) {
                self.blocks[0][0] ^= 1;
            }
            let mut payload = [0; 32];
            payload[..2].copy_from_slice(&raw);
            let mut s = BlockSnapshot {
                address,
                declared_len: 32,
                payload_len: 32,
                payload,
            };
            if i == 0 {
                if matches!(self.fault, Fault::EchoAddress) {
                    s.address += 1;
                }
                if matches!(self.fault, Fault::EchoLength) {
                    s.payload_len = 31;
                }
                if matches!(self.fault, Fault::EchoRaw) {
                    s.payload[0] ^= 1;
                }
            }
            Ok(s)
        }
    }
    #[test]
    fn prerequisites_require_disabled_charging_four_cells_and_pack_termination() {
        assert!(prerequisites_ok(false, 0x8127, 0xD0FE));
        assert!(!prerequisites_ok(true, 0x8127, 0xD0FE));
        for count in 0..3 {
            assert!(!prerequisites_ok(false, 0x8124 | count, 0xD0FE));
        }
        assert!(!prerequisites_ok(false, 0x8127, 0xD0FE | (1 << 9)));
    }
    #[test]
    fn restore_overlay_matches_targets_without_touching_other_bytes() {
        let mut image = [0xa5; 8192];
        assert!(patch_restore_overlay(&mut image));
        for (i, &byte) in image.iter().enumerate() {
            let expected = FIELDS
                .iter()
                .find_map(|f| {
                    let offset = (f.address - 0x4000) as usize;
                    if i == offset {
                        Some(f.raw[0])
                    } else if i == offset + 1 {
                        Some(f.raw[1])
                    } else {
                        None
                    }
                })
                .unwrap_or(0xa5);
            assert_eq!(byte, expected);
        }
        let mut short = [0xa5; 32];
        assert!(!patch_restore_overlay(&mut short));
        assert_eq!(short, [0xa5; 32]);
    }
    #[test]
    fn two_blocks_change_only_the_two_approved_fields() {
        let mut bus = Bus::new(Fault::None);
        let reports = apply_profile(&mut bus).unwrap();
        assert_eq!(bus.writes, 2);
        assert_eq!(bus.reads, 6);
        assert_eq!(FIELDS[0].raw, [0x88, 0x13]);
        assert_eq!(FIELDS[1].raw, [0xc0, 0x2b]);
        for i in 0..2 {
            assert_eq!(reports[i].snapshot.payload, [0xa5; 32]);
            for j in 0..32 {
                let offset = FIELDS[i].offset as usize;
                assert_eq!(
                    bus.blocks[i][j],
                    if j == offset {
                        FIELDS[i].raw[0]
                    } else if j == offset + 1 {
                        FIELDS[i].raw[1]
                    } else {
                        0xa5
                    }
                );
            }
        }
    }
    #[test]
    fn preflight_failure_cannot_write_either_field() {
        for fault in [
            Fault::PreflightTransport,
            Fault::PreflightAddress,
            Fault::PreflightLength,
        ] {
            let mut bus = Bus::new(fault);
            let e = apply_profile(&mut bus).unwrap_err();
            assert_eq!(bus.writes, 0);
            assert!(e.snapshots[0].is_some());
            assert_eq!(e.block, BLOCKS[1]);
        }
    }
    #[test]
    fn partial_failure_never_rolls_back_or_reports_success() {
        for fault in [
            Fault::Write,
            Fault::EchoAddress,
            Fault::EchoLength,
            Fault::EchoRaw,
            Fault::Collateral,
            Fault::LateCollateral,
            Fault::FinalTransport,
            Fault::FinalAddress,
            Fault::FinalLength,
        ] {
            let mut bus = Bus::new(fault);
            let e = apply_profile(&mut bus).unwrap_err();
            assert!(e.snapshots.iter().all(Option::is_some));
            assert!(bus.writes <= 2);
            assert_ne!(e.as_str(), "ok");
        }
    }
}
