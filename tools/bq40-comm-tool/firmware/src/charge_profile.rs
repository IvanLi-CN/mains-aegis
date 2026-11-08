//! Strict BQ40 charge-temperature Data Flash profile.
//!
//! The transport owns the SMBus/MB44 framing. This module owns the allowlist,
//! containing-block snapshot contract, and fail-closed verification sequence.

#![allow(dead_code)]

pub const CONTAINING_BLOCK_ADDRESS: u16 = 0x4A00;
pub const CONTAINING_BLOCK_LEN: u8 = 32;
pub const FIELD_COUNT: usize = 7;
pub const MB44_RESPONSE_LEN: usize = 35;
pub const MB44_DECLARED_LEN_WITH_ADDRESS: u8 = 34;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockReadDecodeError {
    Length,
    Address,
}

pub const fn build_mb44_read_request(address: u16) -> [u8; 4] {
    let address_le = address.to_le_bytes();
    [0x44, 0x02, address_le[0], address_le[1]]
}

/// Build the direct HPE=0 MB44 write used for one allowlisted two-byte field.
/// Keeping this frame in the pure contract module lets the target adapter and
/// host tests exercise the same address/length/raw-byte geometry.
pub const fn build_mb44_field_write_request(address: u16, raw: [u8; 2]) -> [u8; 6] {
    let address_le = address.to_le_bytes();
    [0x44, 0x04, address_le[0], address_le[1], raw[0], raw[1]]
}

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

/// Decode the plain HPE=0 MB44 response used by the charge-temperature
/// profile. The response is a length byte, echoed address, and 32-byte block.
pub fn decode_plain_mb44_block(
    address: u16,
    response: &[u8],
) -> Result<BlockSnapshot, BlockReadDecodeError> {
    if response.len() != MB44_RESPONSE_LEN || response[0] != MB44_DECLARED_LEN_WITH_ADDRESS {
        return Err(BlockReadDecodeError::Length);
    }
    let echoed_address = u16::from_le_bytes([response[1], response[2]]);
    if echoed_address != address {
        return Err(BlockReadDecodeError::Address);
    }
    let mut payload = [0u8; CONTAINING_BLOCK_LEN as usize];
    payload.copy_from_slice(&response[3..MB44_RESPONSE_LEN]);
    Ok(BlockSnapshot {
        address: echoed_address,
        declared_len: CONTAINING_BLOCK_LEN,
        payload_len: CONTAINING_BLOCK_LEN,
        payload,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldVerification {
    pub address: u16,
    pub declared_len: u8,
    pub payload_len: u8,
    pub raw: [u8; 2],
    pub decoded: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyReport<const N: usize = FIELD_COUNT> {
    pub snapshot: BlockSnapshot,
    pub final_snapshot: BlockSnapshot,
    pub field_verifications: [FieldVerification; N],
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
    apply_definition(transport, CONTAINING_BLOCK_ADDRESS, FIELDS)
}

pub(crate) fn apply_definition<T: Transport, const N: usize>(
    transport: &mut T,
    block_address: u16,
    fields: [Field; N],
) -> Result<ApplyReport<N>, ApplyError<T::Error>> {
    let snapshot = match transport.read_block(block_address) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Err(ApplyError::new(
                ApplyErrorKind::SnapshotTransport(error),
                None,
            ));
        }
    };
    if snapshot.address != block_address {
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
    let mut field_verifications = [FieldVerification {
        address: 0,
        declared_len: 0,
        payload_len: 0,
        raw: [0, 0],
        decoded: 0,
    }; N];
    for (index, field) in fields.into_iter().enumerate() {
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
        let raw = [echoed.payload[0], echoed.payload[1]];
        field_verifications[index] = FieldVerification {
            address: echoed.address,
            declared_len: echoed.declared_len,
            payload_len: echoed.payload_len,
            raw,
            decoded: u16::from_le_bytes(raw),
        };
        writes = writes.saturating_add(1);
    }

    let final_snapshot = match transport.read_block(block_address) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Err(ApplyError::new(
                ApplyErrorKind::FinalTransport(error),
                Some(snapshot),
            ));
        }
    };
    if final_snapshot.address != block_address {
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
        let expected = fields.iter().find_map(|field| {
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
    for field in fields {
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
        field_verifications,
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
        SnapshotTransport,
        SnapshotAddress,
        SnapshotLength,
        WriteTransport(usize),
        EchoAddress(usize),
        EchoLength(usize),
        EchoRaw(usize),
        FinalTransport,
        FinalAddress,
        FinalLength,
        FinalRaw,
        FinalUnrelatedRaw,
    }

    struct MockTransport {
        block: [u8; CONTAINING_BLOCK_LEN as usize],
        writes: usize,
        snapshots: usize,
        failure: Failure,
    }

    #[test]
    fn mb44_request_and_plain_response_use_the_expected_frame() {
        let address = CONTAINING_BLOCK_ADDRESS;
        assert_eq!(build_mb44_read_request(address), [0x44, 0x02, 0x00, 0x4A]);
        assert_eq!(
            build_mb44_field_write_request(0x4A0B, [0xAC, 0x0A]),
            [0x44, 0x04, 0x0B, 0x4A, 0xAC, 0x0A]
        );

        let mut response = [0u8; MB44_RESPONSE_LEN];
        response[0] = MB44_DECLARED_LEN_WITH_ADDRESS;
        response[1..3].copy_from_slice(&address.to_le_bytes());
        for (index, byte) in response[3..].iter_mut().enumerate() {
            *byte = index as u8;
        }
        let snapshot = decode_plain_mb44_block(address, &response).unwrap();
        assert_eq!(snapshot.address, address);
        assert_eq!(snapshot.declared_len, CONTAINING_BLOCK_LEN);
        assert_eq!(snapshot.payload_len, CONTAINING_BLOCK_LEN);
        assert_eq!(snapshot.payload[0], 0);
        assert_eq!(snapshot.payload[31], 31);
    }

    #[test]
    fn mb44_decoder_rejects_short_bad_length_and_wrong_echo() {
        assert_eq!(
            decode_plain_mb44_block(CONTAINING_BLOCK_ADDRESS, &[0; 34]),
            Err(BlockReadDecodeError::Length)
        );

        let mut bad_length = [0u8; MB44_RESPONSE_LEN];
        bad_length[0] = MB44_DECLARED_LEN_WITH_ADDRESS - 1;
        assert_eq!(
            decode_plain_mb44_block(CONTAINING_BLOCK_ADDRESS, &bad_length),
            Err(BlockReadDecodeError::Length)
        );

        let mut bad_address = [0u8; MB44_RESPONSE_LEN];
        bad_address[0] = MB44_DECLARED_LEN_WITH_ADDRESS;
        bad_address[1..3].copy_from_slice(&0x4A01u16.to_le_bytes());
        assert_eq!(
            decode_plain_mb44_block(CONTAINING_BLOCK_ADDRESS, &bad_address),
            Err(BlockReadDecodeError::Address)
        );
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
            if matches!(self.failure, Failure::SnapshotTransport) && self.snapshots == 1 {
                return Err(MockError);
            }
            if matches!(self.failure, Failure::FinalTransport) && self.snapshots > 1 {
                return Err(MockError);
            }
            let mut snapshot = BlockSnapshot {
                address,
                declared_len: CONTAINING_BLOCK_LEN,
                payload_len: CONTAINING_BLOCK_LEN,
                payload: self.block,
            };
            if matches!(self.failure, Failure::SnapshotAddress) && self.snapshots == 1 {
                snapshot.address = address + 1;
            }
            if matches!(self.failure, Failure::SnapshotLength) && self.snapshots == 1 {
                snapshot.payload_len = CONTAINING_BLOCK_LEN - 1;
            }
            if matches!(self.failure, Failure::FinalAddress) && self.snapshots > 1 {
                snapshot.address = address + 1;
            }
            if matches!(self.failure, Failure::FinalLength) && self.snapshots > 1 {
                snapshot.payload_len = CONTAINING_BLOCK_LEN - 1;
            }
            if matches!(self.failure, Failure::FinalRaw) && self.snapshots > 1 {
                snapshot.payload[FIELDS[0].offset as usize] ^= 0x01;
            }
            if matches!(self.failure, Failure::FinalUnrelatedRaw) && self.snapshots > 1 {
                snapshot.payload[0] ^= 0x01;
            }
            Ok(snapshot)
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
    fn fields_match_locked_addresses_and_raw_vectors() {
        let expected = [
            ("t1", 0x4A0B, [0xAC, 0x0A]),
            ("t2", 0x4A0D, [0x42, 0x0B]),
            ("t5", 0x4A0F, [0x74, 0x0B]),
            ("t6", 0x4A11, [0x0A, 0x0C]),
            ("t3", 0x4A13, [0x3C, 0x0C]),
            ("t4", 0x4A15, [0x6E, 0x0C]),
            ("hysteresis", 0x4A17, [0x14, 0x00]),
        ];

        for (field, (name, address, raw)) in FIELDS.iter().zip(expected) {
            assert_eq!(field.name, name);
            assert_eq!(field.address, address);
            assert_eq!(field.raw, raw);
            assert_eq!(field.decoded(), u16::from_le_bytes(raw));
        }
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
        for (field, observed) in FIELDS.into_iter().zip(report.field_verifications) {
            assert_eq!(observed.address, field.address);
            assert_eq!(observed.declared_len, CONTAINING_BLOCK_LEN);
            assert_eq!(observed.payload_len, CONTAINING_BLOCK_LEN);
            assert_eq!(observed.raw, field.raw);
            assert_eq!(observed.decoded, field.decoded());
        }
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
    fn snapshot_and_final_metadata_errors_stop_and_preserve_snapshot() {
        for (failure, expected_error, expected_writes, snapshot_present) in [
            (Failure::SnapshotTransport, "snapshot_transport", 0, false),
            (Failure::SnapshotAddress, "snapshot_address", 0, true),
            (Failure::SnapshotLength, "snapshot_length", 0, true),
            (Failure::FinalTransport, "final_transport", 7, true),
            (Failure::FinalAddress, "final_address", 7, true),
            (Failure::FinalLength, "final_length", 7, true),
        ] {
            let mut transport = MockTransport::new(failure);
            let error = apply_profile(&mut transport).unwrap_err();
            assert_eq!(error.as_str(), expected_error);
            assert_eq!(error.snapshot.is_some(), snapshot_present);
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
