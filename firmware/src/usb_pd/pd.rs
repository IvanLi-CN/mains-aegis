pub const MAX_DATA_OBJECTS: usize = 7;
pub const FIXED_VOLTAGE_STEP_MV: u16 = 50;
pub const FIXED_CURRENT_STEP_MA: u16 = 10;
pub const PPS_APDO_VOLTAGE_STEP_MV: u16 = 100;
pub const PPS_APDO_CURRENT_STEP_MA: u16 = 50;
pub const PPS_RDO_VOLTAGE_STEP_MV: u16 = 20;
pub const PPS_RDO_CURRENT_STEP_MA: u16 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SpecRevision {
    Rev10 = 0,
    Rev20 = 1,
    Rev30 = 2,
}

impl SpecRevision {
    pub const fn bits(self) -> u8 {
        self as u8
    }

    pub const fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => Self::Rev10,
            1 => Self::Rev20,
            _ => Self::Rev30,
        }
    }
}

pub const FUSB302_MAX_SPEC_REVISION: SpecRevision = SpecRevision::Rev20;

pub const fn clamp_fusb302_spec_revision(spec_revision: SpecRevision) -> SpecRevision {
    match spec_revision {
        SpecRevision::Rev10 => SpecRevision::Rev10,
        SpecRevision::Rev20 | SpecRevision::Rev30 => FUSB302_MAX_SPEC_REVISION,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ControlMessageType {
    GoodCrc = 1,
    Accept = 3,
    Reject = 4,
    PsRdy = 6,
    Wait = 12,
    SoftReset = 13,
}

impl ControlMessageType {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            1 => Some(Self::GoodCrc),
            3 => Some(Self::Accept),
            4 => Some(Self::Reject),
            6 => Some(Self::PsRdy),
            12 => Some(Self::Wait),
            13 => Some(Self::SoftReset),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DataMessageType {
    SourceCapabilities = 1,
    Request = 2,
    SinkCapabilities = 4,
}

impl DataMessageType {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            1 => Some(Self::SourceCapabilities),
            2 => Some(Self::Request),
            4 => Some(Self::SinkCapabilities),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageHeader {
    raw: u16,
}

impl MessageHeader {
    pub const fn new(raw: u16) -> Self {
        Self { raw }
    }

    pub const fn raw(self) -> u16 {
        self.raw
    }

    pub const fn object_count(self) -> usize {
        ((self.raw >> 12) & 0x07) as usize
    }

    pub const fn message_id(self) -> u8 {
        ((self.raw >> 9) & 0x07) as u8
    }

    pub const fn spec_revision(self) -> SpecRevision {
        SpecRevision::from_bits(((self.raw >> 6) & 0x03) as u8)
    }

    pub const fn data_role(self) -> bool {
        ((self.raw >> 5) & 0x01) != 0
    }

    pub const fn power_role(self) -> bool {
        ((self.raw >> 8) & 0x01) != 0
    }

    pub const fn type_raw(self) -> u8 {
        (self.raw & 0x1f) as u8
    }

    pub const fn is_data_message(self) -> bool {
        self.object_count() != 0
    }

    pub const fn control_message_type(self) -> Option<ControlMessageType> {
        if self.is_data_message() {
            None
        } else {
            ControlMessageType::from_raw(self.type_raw())
        }
    }

    pub const fn data_message_type(self) -> Option<DataMessageType> {
        if !self.is_data_message() {
            None
        } else {
            DataMessageType::from_raw(self.type_raw())
        }
    }

    pub const fn for_control(
        kind: ControlMessageType,
        message_id: u8,
        spec_revision: SpecRevision,
        power_role_source: bool,
        data_role_dfp: bool,
    ) -> Self {
        let mut raw = (kind as u16) & 0x1f;
        raw |= ((message_id & 0x07) as u16) << 9;
        raw |= ((spec_revision.bits() & 0x03) as u16) << 6;
        raw |= (data_role_dfp as u16) << 5;
        raw |= (power_role_source as u16) << 8;
        Self { raw }
    }

    pub const fn for_data(
        kind: DataMessageType,
        object_count: usize,
        message_id: u8,
        spec_revision: SpecRevision,
        power_role_source: bool,
        data_role_dfp: bool,
    ) -> Self {
        let mut raw = (kind as u16) & 0x1f;
        raw |= ((object_count as u16) & 0x07) << 12;
        raw |= ((message_id & 0x07) as u16) << 9;
        raw |= ((spec_revision.bits() & 0x03) as u16) << 6;
        raw |= (data_role_dfp as u16) << 5;
        raw |= (power_role_source as u16) << 8;
        Self { raw }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub header: MessageHeader,
    payload: [u32; MAX_DATA_OBJECTS],
}

impl Message {
    pub const fn new(header: MessageHeader, payload: [u32; MAX_DATA_OBJECTS]) -> Self {
        Self { header, payload }
    }

    pub const fn object_count(&self) -> usize {
        self.header.object_count()
    }

    pub fn payload(&self) -> &[u32] {
        &self.payload[..self.object_count()]
    }

    pub fn payload_mut(&mut self) -> &mut [u32] {
        let count = self.object_count();
        &mut self.payload[..count]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedSupplyPdo {
    pub voltage_mv: u16,
    pub max_current_ma: u16,
    pub raw: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PpsApdo {
    pub min_voltage_mv: u16,
    pub max_voltage_mv: u16,
    pub max_current_ma: u16,
    pub raw: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerDataObject {
    FixedSupply(FixedSupplyPdo),
    Pps(PpsApdo),
    Unsupported(u32),
}

impl PowerDataObject {
    pub const fn from_raw(raw: u32) -> Self {
        match ((raw >> 30) & 0x03) as u8 {
            0 => Self::FixedSupply(FixedSupplyPdo {
                voltage_mv: (((raw >> 10) & 0x03ff) as u16) * FIXED_VOLTAGE_STEP_MV,
                max_current_ma: ((raw & 0x03ff) as u16) * FIXED_CURRENT_STEP_MA,
                raw,
            }),
            3 if ((raw >> 28) & 0x03) as u8 == 0 => Self::Pps(PpsApdo {
                max_voltage_mv: (((raw >> 17) & 0xff) as u16) * PPS_APDO_VOLTAGE_STEP_MV,
                min_voltage_mv: (((raw >> 8) & 0xff) as u16) * PPS_APDO_VOLTAGE_STEP_MV,
                max_current_ma: ((raw & 0x7f) as u16) * PPS_APDO_CURRENT_STEP_MA,
                raw,
            }),
            _ => Self::Unsupported(raw),
        }
    }

    pub const fn raw(self) -> u32 {
        match self {
            Self::FixedSupply(pdo) => pdo.raw,
            Self::Pps(apdo) => apdo.raw,
            Self::Unsupported(raw) => raw,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestDataObject {
    raw: u32,
}

impl RequestDataObject {
    pub const fn new(raw: u32) -> Self {
        Self { raw }
    }

    pub const fn raw(self) -> u32 {
        self.raw
    }

    pub const fn object_position(self) -> u8 {
        ((self.raw >> 28) & 0x07) as u8
    }

    pub const fn operating_current_ma(self) -> u16 {
        (((self.raw >> 10) & 0x03ff) as u16) * FIXED_CURRENT_STEP_MA
    }

    pub const fn max_operating_current_ma(self) -> u16 {
        ((self.raw & 0x03ff) as u16) * FIXED_CURRENT_STEP_MA
    }

    pub const fn pps_voltage_mv(self) -> u16 {
        (((self.raw >> 9) & 0x0fff) as u16) * PPS_RDO_VOLTAGE_STEP_MV
    }

    pub const fn pps_current_ma(self) -> u16 {
        ((self.raw & 0x7f) as u16) * PPS_RDO_CURRENT_STEP_MA
    }

    pub const fn for_fixed(object_position: u8, operating_current_ma: u16) -> Self {
        let current_field = operating_current_ma.div_ceil(FIXED_CURRENT_STEP_MA) as u32;
        let raw = ((object_position as u32) & 0x07) << 28
            | (1u32 << 24)
            | ((current_field & 0x03ff) << 10)
            | (current_field & 0x03ff);
        Self { raw }
    }

    pub const fn for_pps(object_position: u8, voltage_mv: u16, operating_current_ma: u16) -> Self {
        let voltage_field = voltage_mv.div_ceil(PPS_RDO_VOLTAGE_STEP_MV) as u32;
        let current_field = operating_current_ma.div_ceil(PPS_RDO_CURRENT_STEP_MA) as u32;
        let raw = ((object_position as u32) & 0x07) << 28
            | (1u32 << 24)
            | ((voltage_field & 0x0fff) << 9)
            | (current_field & 0x7f);
        Self { raw }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceCapabilities {
    objects: [PowerDataObject; MAX_DATA_OBJECTS],
    len: usize,
    pub spec_revision: SpecRevision,
}

impl SourceCapabilities {
    pub const fn empty(spec_revision: SpecRevision) -> Self {
        Self {
            objects: [PowerDataObject::Unsupported(0); MAX_DATA_OBJECTS],
            len: 0,
            spec_revision,
        }
    }

    pub fn from_message(message: &Message) -> Option<Self> {
        if message.header.data_message_type() != Some(DataMessageType::SourceCapabilities) {
            return None;
        }
        let mut caps = Self::empty(message.header.spec_revision());
        let payload = message.payload();
        let mut idx = 0;
        while idx < payload.len() {
            caps.objects[idx] = PowerDataObject::from_raw(payload[idx]);
            idx += 1;
        }
        caps.len = payload.len();
        Some(caps)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, index: usize) -> Option<PowerDataObject> {
        (index < self.len).then_some(self.objects[index])
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, PowerDataObject)> + '_ {
        self.objects[..self.len].iter().copied().enumerate()
    }
}

pub const fn clamp_voltage_mv(voltage_mv: u16, min_mv: u16, max_mv: u16) -> u16 {
    if voltage_mv < min_mv {
        min_mv
    } else if voltage_mv > max_mv {
        max_mv
    } else {
        voltage_mv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn fixed_pdo_raw(voltage_mv: u16, current_ma: u16) -> u32 {
        (((voltage_mv / FIXED_VOLTAGE_STEP_MV) as u32) << 10)
            | ((current_ma / FIXED_CURRENT_STEP_MA) as u32)
    }

    const fn pps_apdo_raw(min_mv: u16, max_mv: u16, current_ma: u16) -> u32 {
        (3u32 << 30)
            | (((max_mv / PPS_APDO_VOLTAGE_STEP_MV) as u32) << 17)
            | (((min_mv / PPS_APDO_VOLTAGE_STEP_MV) as u32) << 8)
            | ((current_ma / PPS_APDO_CURRENT_STEP_MA) as u32)
    }

    #[test]
    fn parses_fixed_supply_pdo() {
        let pdo = PowerDataObject::from_raw(fixed_pdo_raw(20_000, 3_000));
        match pdo {
            PowerDataObject::FixedSupply(pdo) => {
                assert_eq!(pdo.voltage_mv, 20_000);
                assert_eq!(pdo.max_current_ma, 3_000);
            }
            other => panic!("unexpected pdo: {other:?}"),
        }
    }

    #[test]
    fn parses_pps_apdo() {
        let pdo = PowerDataObject::from_raw(pps_apdo_raw(5_000, 11_000, 3_000));
        match pdo {
            PowerDataObject::Pps(apdo) => {
                assert_eq!(apdo.min_voltage_mv, 5_000);
                assert_eq!(apdo.max_voltage_mv, 11_000);
                assert_eq!(apdo.max_current_ma, 3_000);
            }
            other => panic!("unexpected apdo: {other:?}"),
        }
    }

    #[test]
    fn encodes_fixed_rdo() {
        let rdo = RequestDataObject::for_fixed(3, 2_000);
        assert_eq!(rdo.object_position(), 3);
        assert_eq!(rdo.operating_current_ma(), 2_000);
        assert_eq!(rdo.max_operating_current_ma(), 2_000);
    }

    #[test]
    fn encodes_pps_rdo() {
        let rdo = RequestDataObject::for_pps(2, 8_400, 2_500);
        assert_eq!(rdo.object_position(), 2);
        assert_eq!(rdo.pps_voltage_mv(), 8_400);
        assert_eq!(rdo.pps_current_ma(), 2_500);
    }

    #[test]
    fn parses_source_capabilities_message() {
        let header = MessageHeader::for_data(
            DataMessageType::SourceCapabilities,
            2,
            1,
            SpecRevision::Rev30,
            true,
            true,
        );
        let message = Message::new(
            header,
            [
                fixed_pdo_raw(5_000, 3_000),
                pps_apdo_raw(5_000, 11_000, 3_000),
                0,
                0,
                0,
                0,
                0,
            ],
        );
        let caps = SourceCapabilities::from_message(&message).unwrap();
        assert_eq!(caps.len(), 2);
        assert_eq!(caps.spec_revision, SpecRevision::Rev30);
        assert!(matches!(caps.get(0), Some(PowerDataObject::FixedSupply(_))));
        assert!(matches!(caps.get(1), Some(PowerDataObject::Pps(_))));
    }

    #[test]
    fn builds_headers() {
        let header = MessageHeader::for_control(
            ControlMessageType::Accept,
            5,
            SpecRevision::Rev20,
            false,
            false,
        );
        assert_eq!(header.message_id(), 5);
        assert_eq!(header.spec_revision(), SpecRevision::Rev20);
        assert_eq!(
            header.control_message_type(),
            Some(ControlMessageType::Accept)
        );
        assert!(!header.is_data_message());
    }

    #[test]
    fn clamps_fusb302_revision_to_pd20() {
        assert_eq!(
            clamp_fusb302_spec_revision(SpecRevision::Rev10),
            SpecRevision::Rev10
        );
        assert_eq!(
            clamp_fusb302_spec_revision(SpecRevision::Rev20),
            SpecRevision::Rev20
        );
        assert_eq!(
            clamp_fusb302_spec_revision(SpecRevision::Rev30),
            SpecRevision::Rev20
        );
    }
}
