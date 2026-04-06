use esp_firmware::ina3221;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputChannel {
    OutA,
    OutB,
}

impl OutputChannel {
    pub const fn addr(self) -> u8 {
        match self {
            OutputChannel::OutA => 0x74,
            OutputChannel::OutB => 0x75,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            OutputChannel::OutA => "out_a",
            OutputChannel::OutB => "out_b",
        }
    }

    pub const fn ina_ch(self) -> ina3221::Channel {
        match self {
            OutputChannel::OutA => ina3221::Channel::Ch2,
            OutputChannel::OutB => ina3221::Channel::Ch1,
        }
    }

    pub const fn tmp_addr(self) -> u8 {
        match self {
            OutputChannel::OutA => 0x48,
            OutputChannel::OutB => 0x49,
        }
    }
}
