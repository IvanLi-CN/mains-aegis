//! Strict BQ40 charge-temperature Data Flash profile.
//!
//! The transport owns the SMBus/MB44 framing. This module owns the allowlist,
//! containing-block snapshot contract, and fail-closed verification sequence.

#![allow(dead_code)]

pub const CONTAINING_BLOCK_ADDRESS: u16 = 0x4A00;
pub const CONTAINING_BLOCK_LEN: u8 = 32;
pub const FIELD_COUNT: usize = 7;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub address: u16,
    pub offset: u8,
    pub raw: [u8; 2],
}

impl Field {
    pub const fn new(name: &'static str, address: u16, raw: [u8; 2]) -> Self {
        Self {
            name,
            address,
            offset: (address - CONTAINING_BLOCK_ADDRESS) as u8,
            raw,
        }
    }

    pub const fn decoded(self) -> u16 {
        u16::from_le_bytes(self.raw)
    }
}

pub const FIELDS: [Field; FIELD_COUNT] = [
    Field::new("t1", 0x4A0B, [0xAC, 0x0A]),
    Field::new("t2", 0x4A0D, [0x42, 0x0B]),
    Field::new("t5", 0x4A0F, [0x74, 0x0B]),
    Field::new("t6", 0x4A11, [0x0A, 0x0C]),
    Field::new("t3", 0x4A13, [0x3C, 0x0C]),
    Field::new("t4", 0x4A15, [0x6E, 0x0C]),
    Field::new("hysteresis", 0x4A17, [0x14, 0x00]),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockSnapshot {
    pub address: u16,
    pub declared_len: u8,
    pub payload_len: u8,
    pub payload: [u8; CONTAINING_BLOCK_LEN as usize],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyReport {
    pub snapshot: BlockSnapshot,
    pub final_snapshot: BlockSnapshot,
    pub writes: u8,
}

pub trait Transport {
    type Error: Copy;

    /// Read a 32-byte MB44 Data Flash block and return its echoed address.
    fn read_block(&mut self, address: u16) -> Result<BlockSnapshot, Self::Error>;

    /// Write one two-byte field, then read back the corresponding MB44 block.
    fn write_field(&mut self, address: u16, raw: [u8; 2]) -> Result<BlockSnapshot, Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyErrorKind<E: Copy> {
    SnapshotTransport(E),
    SnapshotAddress,
    SnapshotLength,
    WriteTransport(E),
    EchoAddress,
    EchoLength,
    EchoRaw,
    EchoDecoded,
    FinalTransport(E),
    FinalAddress,
    FinalLength,
    FinalRaw,
    FinalDecoded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyError<E: Copy> {
    pub kind: ApplyErrorKind<E>,
    /// The complete pre-write containing block when transport reached it.
    /// Keeping it on failures makes the non-atomic operation auditable and
    /// leaves a caller with the bytes needed for a future explicit rollback.
    pub snapshot: Option<BlockSnapshot>,
}

impl<E: Copy> ApplyError<E> {
    pub const fn as_str(self) -> &'static str {
        match self.kind {
            ApplyErrorKind::SnapshotTransport(_) => "snapshot_transport",
            ApplyErrorKind::SnapshotAddress => "snapshot_address",
            ApplyErrorKind::SnapshotLength => "snapshot_length",
            ApplyErrorKind::WriteTransport(_) => "write_transport",
            ApplyErrorKind::EchoAddress => "echo_address",
            ApplyErrorKind::EchoLength => "echo_length",
            ApplyErrorKind::EchoRaw => "echo_raw",
            ApplyErrorKind::EchoDecoded => "echo_decoded",
            ApplyErrorKind::FinalTransport(_) => "final_transport",
            ApplyErrorKind::FinalAddress => "final_address",
            ApplyErrorKind::FinalLength => "final_length",
            ApplyErrorKind::FinalRaw => "final_raw",
            ApplyErrorKind::FinalDecoded => "final_decoded",
        }
    }

    const fn new(kind: ApplyErrorKind<E>, snapshot: Option<BlockSnapshot>) -> Self {
        Self { kind, snapshot }
    }
}

pub fn apply_profile<T: Transport>(transport: &mut T) -> Result<ApplyReport, ApplyError<T::Error>> {
    let snapshot = match transport.read_block(CONTAINING_BLOCK_ADDRESS) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Err(ApplyError::new(
                ApplyErrorKind::SnapshotTransport(error),
                None,
            ));
        }
    };
    if snapshot.address != CONTAINING_BLOCK_ADDRESS {
        return Err(ApplyError::new(
            ApplyErrorKind::SnapshotAddress,
            Some(snapshot),
        ));
    }
    if snapshot.declared_len != CONTAINING_BLOCK_LEN || snapshot.payload_len != CONTAINING_BLOCK_LEN
    {
        return Err(ApplyError::new(
            ApplyErrorKind::SnapshotLength,
            Some(snapshot),
        ));
    }

    let mut writes = 0u8;
    for field in FIELDS {
        let echoed = match transport.write_field(field.address, field.raw) {
            Ok(echoed) => echoed,
            Err(error) => {
                return Err(ApplyError::new(
                    ApplyErrorKind::WriteTransport(error),
                    Some(snapshot),
                ));
            }
        };
        if echoed.address != field.address {
            return Err(ApplyError::new(ApplyErrorKind::EchoAddress, Some(snapshot)));
        }
        if echoed.declared_len != CONTAINING_BLOCK_LEN || echoed.payload_len != CONTAINING_BLOCK_LEN
        {
            return Err(ApplyError::new(ApplyErrorKind::EchoLength, Some(snapshot)));
        }
        if echoed.payload[..2] != field.raw {
            return Err(ApplyError::new(ApplyErrorKind::EchoRaw, Some(snapshot)));
        }
        if u16::from_le_bytes([echoed.payload[0], echoed.payload[1]]) != field.decoded() {
            return Err(ApplyError::new(ApplyErrorKind::EchoDecoded, Some(snapshot)));
        }
        writes = writes.saturating_add(1);
    }

    let final_snapshot = match transport.read_block(CONTAINING_BLOCK_ADDRESS) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Err(ApplyError::new(
                ApplyErrorKind::FinalTransport(error),
                Some(snapshot),
            ));
        }
    };
    if final_snapshot.address != CONTAINING_BLOCK_ADDRESS {
        return Err(ApplyError::new(
            ApplyErrorKind::FinalAddress,
            Some(snapshot),
        ));
    }
    if final_snapshot.declared_len != CONTAINING_BLOCK_LEN
        || final_snapshot.payload_len != CONTAINING_BLOCK_LEN
    {
        return Err(ApplyError::new(ApplyErrorKind::FinalLength, Some(snapshot)));
    }
    for (index, (&before, &after)) in snapshot
        .payload
        .iter()
        .zip(final_snapshot.payload.iter())
        .enumerate()
    {
        let expected = FIELDS.iter().find_map(|field| {
            let offset = field.offset as usize;
            match index.checked_sub(offset) {
                Some(0) => Some(field.raw[0]),
                Some(1) => Some(field.raw[1]),
                _ => None,
            }
        });
        if after != expected.unwrap_or(before) {
            return Err(ApplyError::new(ApplyErrorKind::FinalRaw, Some(snapshot)));
        }
    }
    for field in FIELDS {
        let offset = field.offset as usize;
        if final_snapshot.payload[offset..offset + 2] != field.raw {
            return Err(ApplyError::new(ApplyErrorKind::FinalRaw, Some(snapshot)));
        }
        if u16::from_le_bytes([
            final_snapshot.payload[offset],
            final_snapshot.payload[offset + 1],
        ]) != field.decoded()
        {
            return Err(ApplyError::new(
                ApplyErrorKind::FinalDecoded,
                Some(snapshot),
            ));
        }
    }

    Ok(ApplyReport {
        snapshot,
        final_snapshot,
        writes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct MockError;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Failure {
        None,
        WriteTransport(usize),
        EchoAddress(usize),
        EchoLength(usize),
        EchoRaw(usize),
        FinalRaw,
        FinalUnrelatedRaw,
    }

    struct MockTransport {
        block: [u8; CONTAINING_BLOCK_LEN as usize],
        writes: usize,
        snapshots: usize,
        failure: Failure,
    }

    impl MockTransport {
        fn new(failure: Failure) -> Self {
            Self {
                block: [0xA5; CONTAINING_BLOCK_LEN as usize],
                writes: 0,
                snapshots: 0,
                failure,
            }
        }

        fn field_snapshot(&self, address: u16) -> BlockSnapshot {
            let offset = (address - CONTAINING_BLOCK_ADDRESS) as usize;
            let mut payload = [0u8; CONTAINING_BLOCK_LEN as usize];
            let available = self.block.len().saturating_sub(offset);
            payload[..available].copy_from_slice(&self.block[offset..]);
            BlockSnapshot {
                address,
                declared_len: CONTAINING_BLOCK_LEN,
                payload_len: CONTAINING_BLOCK_LEN,
                payload,
            }
        }
    }

    impl Transport for MockTransport {
        type Error = MockError;

        fn read_block(&mut self, address: u16) -> Result<BlockSnapshot, Self::Error> {
            self.snapshots += 1;
            if matches!(self.failure, Failure::FinalRaw) && self.snapshots > 1 {
                let mut snapshot = BlockSnapshot {
                    address,
                    declared_len: CONTAINING_BLOCK_LEN,
                    payload_len: CONTAINING_BLOCK_LEN,
                    payload: self.block,
                };
                snapshot.payload[FIELDS[0].offset as usize] ^= 0x01;
                return Ok(snapshot);
            }
            if matches!(self.failure, Failure::FinalUnrelatedRaw) && self.snapshots > 1 {
                let mut snapshot = BlockSnapshot {
                    address,
                    declared_len: CONTAINING_BLOCK_LEN,
                    payload_len: CONTAINING_BLOCK_LEN,
                    payload: self.block,
                };
                snapshot.payload[0] ^= 0x01;
                return Ok(snapshot);
            }
            Ok(BlockSnapshot {
                address,
                declared_len: CONTAINING_BLOCK_LEN,
                payload_len: CONTAINING_BLOCK_LEN,
                payload: self.block,
            })
        }

        fn write_field(
            &mut self,
            address: u16,
            raw: [u8; 2],
        ) -> Result<BlockSnapshot, Self::Error> {
            let index = self.writes;
            if matches!(self.failure, Failure::WriteTransport(at) if at == index) {
                return Err(MockError);
            }
            let offset = (address - CONTAINING_BLOCK_ADDRESS) as usize;
            self.block[offset..offset + 2].copy_from_slice(&raw);
            self.writes += 1;
            let mut snapshot = self.field_snapshot(address);
            match self.failure {
                Failure::EchoAddress(at) if at == index => snapshot.address = address + 1,
                Failure::EchoLength(at) if at == index => snapshot.payload_len = 31,
                Failure::EchoRaw(at) if at == index => {
                    snapshot.payload[0] ^= 0x01;
                }
                _ => {}
            }
            Ok(snapshot)
        }
    }

    #[test]
    fn allowlist_is_exactly_seven_temperature_fields() {
        assert_eq!(FIELDS.len(), FIELD_COUNT);
        assert_eq!(FIELD_COUNT, 7);
        assert!(FIELDS.iter().all(|field| field.address >= 0x4A0B));
        assert!(FIELDS.iter().all(|field| field.address <= 0x4A17));
        assert!(!FIELDS.iter().any(|field| field.name.contains("precharge")));
        assert!(!FIELDS
            .iter()
            .any(|field| field.name.contains("termination")));
        assert!(!FIELDS.iter().any(|field| field.name.contains("protection")));
    }

    #[test]
    fn apply_saves_full_snapshot_and_verifies_every_field() {
        let mut transport = MockTransport::new(Failure::None);
        let report = match apply_profile(&mut transport) {
            Ok(report) => report,
            Err(error) => panic!("unexpected profile error: {}", error.as_str()),
        };
        assert_eq!(report.snapshot.address, CONTAINING_BLOCK_ADDRESS);
        assert_eq!(report.snapshot.payload[0], 0xA5);
        assert_eq!(report.final_snapshot.address, CONTAINING_BLOCK_ADDRESS);
        assert_eq!(report.writes, 7);
        assert_eq!(transport.writes, 7);
        assert_eq!(transport.snapshots, 2);
    }

    #[test]
    fn verification_error_stops_without_fallback() {
        for (failure, expected_writes) in [
            (Failure::WriteTransport(2), 2),
            (Failure::EchoAddress(2), 3),
            (Failure::EchoLength(2), 3),
            (Failure::EchoRaw(2), 3),
        ] {
            let mut transport = MockTransport::new(failure);
            let error = apply_profile(&mut transport).unwrap_err();
            assert!(error.snapshot.is_some());
            assert_eq!(transport.writes, expected_writes);
        }
    }

    #[test]
    fn final_containing_block_mismatch_stops_after_all_writes() {
        let mut transport = MockTransport::new(Failure::FinalRaw);
        assert!(matches!(
            apply_profile(&mut transport),
            Err(ApplyError {
                kind: ApplyErrorKind::FinalRaw,
                snapshot: Some(_),
            })
        ));
        assert_eq!(transport.writes, 7);
    }

    #[test]
    fn final_containing_block_unrelated_mismatch_stops_after_all_writes() {
        let mut transport = MockTransport::new(Failure::FinalUnrelatedRaw);
        assert!(matches!(
            apply_profile(&mut transport),
            Err(ApplyError {
                kind: ApplyErrorKind::FinalRaw,
                snapshot: Some(_),
            })
        ));
        assert_eq!(transport.writes, 7);
    }
}
