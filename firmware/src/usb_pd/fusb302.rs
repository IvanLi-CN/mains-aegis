use super::pd::{Message, MessageHeader, SpecRevision, MAX_DATA_OBJECTS};

pub const I2C_ADDRESS: u8 = 0x22;
const FIFO_TX_FRAME_MAX: usize = 40;

const TOKEN_TX_ON: u8 = 0xA1;
const TOKEN_SOP1: u8 = 0x12;
const TOKEN_SOP2: u8 = 0x13;
const TOKEN_RESET1: u8 = 0x15;
const TOKEN_RESET2: u8 = 0x16;
const TOKEN_PACKSYM: u8 = 0x80;
const TOKEN_JAM_CRC: u8 = 0xFF;
const TOKEN_EOP: u8 = 0x14;
const TOKEN_TX_OFF: u8 = 0xFE;

const SOP_TOKEN_MASK: u8 = 0b1110_0000;
const SOP_TOKEN_SOP: u8 = 0b1110_0000;

const POWER_ALL: u8 = 0x0F;
const HOST_CUR_DEFAULT: u8 = 0b01 << 2;
const SWITCHES0_PDWN_BOTH: u8 = switches0::PDWN1 | switches0::PDWN2;
const CONTROL3_BASE: u8 = control3::AUTO_RETRY
    | control3::N_RETRIES_3
    | control3::AUTO_SOFT_RESET
    | control3::AUTO_HARD_RESET;

pub mod reg {
    pub const DEVICE_ID: u8 = 0x01;
    pub const SWITCHES0: u8 = 0x02;
    pub const SWITCHES1: u8 = 0x03;
    pub const MEASURE: u8 = 0x04;
    pub const CONTROL0: u8 = 0x06;
    pub const CONTROL1: u8 = 0x07;
    pub const CONTROL2: u8 = 0x08;
    pub const CONTROL3: u8 = 0x09;
    pub const MASK: u8 = 0x0A;
    pub const POWER: u8 = 0x0B;
    pub const RESET: u8 = 0x0C;
    pub const MASKA: u8 = 0x0E;
    pub const MASKB: u8 = 0x0F;
    pub const STATUS0A: u8 = 0x3C;
    pub const STATUS1A: u8 = 0x3D;
    pub const INTERRUPTA: u8 = 0x3E;
    pub const INTERRUPTB: u8 = 0x3F;
    pub const STATUS0: u8 = 0x40;
    pub const STATUS1: u8 = 0x41;
    pub const INTERRUPT: u8 = 0x42;
    pub const FIFOS: u8 = 0x43;
}

pub mod switches0 {
    pub const PU_EN2: u8 = 1 << 7;
    pub const PU_EN1: u8 = 1 << 6;
    pub const VCONN_CC2: u8 = 1 << 5;
    pub const VCONN_CC1: u8 = 1 << 4;
    pub const MEAS_CC2: u8 = 1 << 3;
    pub const MEAS_CC1: u8 = 1 << 2;
    pub const PDWN2: u8 = 1 << 1;
    pub const PDWN1: u8 = 1 << 0;
}

pub mod switches1 {
    pub const POWER_ROLE_SOURCE: u8 = 1 << 7;
    pub const SPECREV_SHIFT: u8 = 5;
    pub const DATA_ROLE_DFP: u8 = 1 << 4;
    pub const AUTO_GCRC: u8 = 1 << 2;
    pub const TXCC2: u8 = 1 << 1;
    pub const TXCC1: u8 = 1 << 0;
}

pub mod control0 {
    pub const TX_FLUSH: u8 = 1 << 6;
    pub const INT_MASK: u8 = 1 << 5;
    pub const HOST_CUR_SHIFT: u8 = 2;
    pub const TX_START: u8 = 1 << 0;
}

pub mod control1 {
    pub const ENSOP2DB: u8 = 1 << 6;
    pub const ENSOP1DB: u8 = 1 << 5;
    pub const BIST_MODE2: u8 = 1 << 4;
    pub const RX_FLUSH: u8 = 1 << 2;
    pub const ENSOP2: u8 = 1 << 1;
    pub const ENSOP1: u8 = 1 << 0;
}

pub mod control2 {
    pub const TOG_SAVE_PWR2: u8 = 1 << 7;
    pub const TOG_SAVE_PWR1: u8 = 1 << 6;
    pub const TOG_RD_ONLY: u8 = 1 << 5;
    pub const WAKE_EN: u8 = 1 << 3;
    pub const MODE_UFP: u8 = 0x04;
    pub const MODE_DFP: u8 = 0x02;
    pub const TOGGLE: u8 = 1 << 0;
}

pub mod control3 {
    pub const SEND_HARD_RESET: u8 = 1 << 6;
    pub const BIST_TMODE: u8 = 1 << 5;
    pub const AUTO_HARD_RESET: u8 = 1 << 4;
    pub const AUTO_SOFT_RESET: u8 = 1 << 3;
    pub const N_RETRIES_3: u8 = 0x06;
    pub const AUTO_RETRY: u8 = 1 << 0;
}

pub mod reset {
    pub const PD_RESET: u8 = 1 << 1;
    pub const SW_RESET: u8 = 1 << 0;
}

pub mod status0a {
    pub const SOFT_FAIL: u8 = 1 << 5;
    pub const RETRY_FAIL: u8 = 1 << 4;
}

pub mod status1a {
    pub const TOGS_SHIFT: u8 = 3;
    pub const TOGS_MASK: u8 = 0b111 << TOGS_SHIFT;
    pub const TOGS_SNK1: u8 = 0b101 << TOGS_SHIFT;
    pub const TOGS_SNK2: u8 = 0b110 << TOGS_SHIFT;
    pub const RXSOP2DB: u8 = 1 << 2;
    pub const RXSOP1DB: u8 = 1 << 1;
    pub const RXSOP: u8 = 1 << 0;
}

pub mod interrupta {
    pub const TOG_DONE: u8 = 1 << 6;
    pub const SOFT_FAIL: u8 = 1 << 5;
    pub const RETRY_FAIL: u8 = 1 << 4;
    pub const HARD_SENT: u8 = 1 << 3;
    pub const TX_SENT: u8 = 1 << 2;
    pub const SOFT_RESET: u8 = 1 << 1;
    pub const HARD_RESET: u8 = 1 << 0;
}

pub mod interruptb {
    pub const GCRC_SENT: u8 = 1 << 0;
}

pub mod status0 {
    pub const VBUS_OK: u8 = 1 << 7;
    pub const ACTIVITY: u8 = 1 << 6;
    pub const CRC_CHK: u8 = 1 << 4;
}

pub mod status1 {
    pub const RX_EMPTY: u8 = 1 << 5;
    pub const RX_FULL: u8 = 1 << 4;
    pub const TX_EMPTY: u8 = 1 << 3;
    pub const TX_FULL: u8 = 1 << 2;
}

pub mod interrupt {
    pub const VBUS_OK: u8 = 1 << 7;
    pub const ACTIVITY: u8 = 1 << 6;
    pub const CRC_CHK: u8 = 1 << 4;
    pub const ALERT: u8 = 1 << 3;
    pub const WAKE: u8 = 1 << 2;
    pub const COLLISION: u8 = 1 << 1;
    pub const BC_LVL: u8 = 1 << 0;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CcPolarity {
    Cc1,
    Cc2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrqSnapshot {
    pub status0a: u8,
    pub status1a: u8,
    pub interrupta: u8,
    pub interruptb: u8,
    pub status0: u8,
    pub status1: u8,
    pub interrupt: u8,
}

impl IrqSnapshot {
    pub fn attached_sink_polarity(&self) -> Option<CcPolarity> {
        match self.status1a & status1a::TOGS_MASK {
            status1a::TOGS_SNK1 => Some(CcPolarity::Cc1),
            status1a::TOGS_SNK2 => Some(CcPolarity::Cc2),
            _ => None,
        }
    }

    pub const fn vbus_present(&self) -> bool {
        (self.status0 & status0::VBUS_OK) != 0
    }

    pub const fn retry_failed(&self) -> bool {
        (self.interrupta & interrupta::RETRY_FAIL) != 0
            || (self.status0a & status0a::RETRY_FAIL) != 0
    }

    pub const fn soft_reset_received(&self) -> bool {
        (self.interrupta & interrupta::SOFT_RESET) != 0
    }

    pub const fn hard_reset_received(&self) -> bool {
        (self.interrupta & interrupta::HARD_RESET) != 0
    }

    pub const fn tx_sent(&self) -> bool {
        (self.interrupta & interrupta::TX_SENT) != 0
    }

    pub const fn gcrc_sent(&self) -> bool {
        (self.interruptb & interruptb::GCRC_SENT) != 0
    }

    pub const fn rx_message_ready(&self) -> bool {
        (self.status1 & status1::RX_EMPTY) == 0
            && (self.status0 & status0::CRC_CHK) != 0
            && (self.status1a & status1a::RXSOP) != 0
    }
}

#[derive(Debug)]
pub enum Error {
    I2c(esp_hal::i2c::master::Error),
    Protocol(&'static str),
}

impl From<esp_hal::i2c::master::Error> for Error {
    fn from(value: esp_hal::i2c::master::Error) -> Self {
        Self::I2c(value)
    }
}

pub struct Fusb302<I2C> {
    i2c: I2C,
    switches1_base: u8,
}

impl<I2C> Fusb302<I2C>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    pub fn new(i2c: I2C) -> Self {
        Self {
            i2c,
            switches1_base: 0,
        }
    }

    pub fn init_sink(&mut self, spec_revision: SpecRevision) -> Result<u8, Error> {
        self.write_reg(reg::RESET, reset::SW_RESET)?;
        self.write_reg(reg::RESET, reset::PD_RESET)?;
        self.write_reg(reg::POWER, POWER_ALL)?;
        self.write_reg(reg::CONTROL0, HOST_CUR_DEFAULT)?;
        self.write_reg(reg::CONTROL1, control1::RX_FLUSH)?;
        self.write_reg(reg::CONTROL3, CONTROL3_BASE)?;
        self.write_reg(reg::MASK, 0x00)?;
        self.write_reg(reg::MASKA, 0x00)?;
        self.write_reg(reg::MASKB, 0x00)?;
        self.switches1_base = (spec_revision.bits() << switches1::SPECREV_SHIFT) & 0x60;
        self.write_reg(reg::SWITCHES1, self.switches1_base)?;
        self.write_reg(reg::SWITCHES0, SWITCHES0_PDWN_BOTH)?;
        self.write_reg(reg::MEASURE, 0x00)?;
        let _ = self.poll_status()?;
        self.start_sink_toggle()?;
        self.read_reg(reg::DEVICE_ID).map_err(Error::from)
    }

    pub fn start_sink_toggle(&mut self) -> Result<(), Error> {
        self.write_reg(reg::SWITCHES0, SWITCHES0_PDWN_BOTH)?;
        self.write_reg(reg::SWITCHES1, self.switches1_base)?;
        self.write_reg(reg::CONTROL2, control2::MODE_UFP | control2::TOGGLE)?;
        Ok(())
    }

    pub fn configure_sink_polarity(
        &mut self,
        polarity: CcPolarity,
        spec_revision: SpecRevision,
    ) -> Result<(), Error> {
        self.write_reg(reg::CONTROL2, control2::MODE_UFP)?;
        let switches0 = match polarity {
            CcPolarity::Cc1 => SWITCHES0_PDWN_BOTH | switches0::MEAS_CC1,
            CcPolarity::Cc2 => SWITCHES0_PDWN_BOTH | switches0::MEAS_CC2,
        };
        let tx_cc = match polarity {
            CcPolarity::Cc1 => switches1::TXCC1,
            CcPolarity::Cc2 => switches1::TXCC2,
        };
        self.switches1_base = (spec_revision.bits() << switches1::SPECREV_SHIFT) & 0x60;
        self.write_reg(reg::SWITCHES0, switches0)?;
        self.write_reg(reg::SWITCHES1, self.switches1_base | tx_cc)?;
        Ok(())
    }

    pub fn enable_pd_receive(
        &mut self,
        polarity: CcPolarity,
        spec_revision: SpecRevision,
    ) -> Result<(), Error> {
        self.flush_rx()?;
        self.flush_tx()?;
        self.configure_sink_polarity(polarity, spec_revision)?;
        let tx_cc = match polarity {
            CcPolarity::Cc1 => switches1::TXCC1,
            CcPolarity::Cc2 => switches1::TXCC2,
        };
        self.write_reg(
            reg::SWITCHES1,
            self.switches1_base | tx_cc | switches1::AUTO_GCRC,
        )?;
        Ok(())
    }

    pub fn flush_rx(&mut self) -> Result<(), Error> {
        self.write_reg(reg::CONTROL1, control1::RX_FLUSH)?;
        Ok(())
    }

    pub fn flush_tx(&mut self) -> Result<(), Error> {
        let cur = self.read_reg(reg::CONTROL0)?;
        self.write_reg(reg::CONTROL0, cur | control0::TX_FLUSH)?;
        Ok(())
    }

    pub fn poll_status(&mut self) -> Result<IrqSnapshot, Error> {
        let mut buf = [0u8; 7];
        self.read_block(reg::STATUS0A, &mut buf)?;
        Ok(IrqSnapshot {
            status0a: buf[0],
            status1a: buf[1],
            interrupta: buf[2],
            interruptb: buf[3],
            status0: buf[4],
            status1: buf[5],
            interrupt: buf[6],
        })
    }

    pub fn read_message(&mut self) -> Result<Option<Message>, Error> {
        let sop_token = self.read_fifo_byte()?;
        if (sop_token & SOP_TOKEN_MASK) != SOP_TOKEN_SOP {
            return Err(Error::Protocol("unsupported_sop_token"));
        }

        let mut header_buf = [0u8; 2];
        self.read_fifo_bytes(&mut header_buf)?;
        let header = MessageHeader::new(u16::from_le_bytes(header_buf));
        if header.object_count() > MAX_DATA_OBJECTS {
            return Err(Error::Protocol("pdo_count_overflow"));
        }

        let mut payload = [0u32; MAX_DATA_OBJECTS];
        let payload_len = header.object_count() * 4;
        if payload_len != 0 {
            let mut raw_payload = [0u8; MAX_DATA_OBJECTS * 4];
            self.read_fifo_bytes(&mut raw_payload[..payload_len])?;
            for (idx, chunk) in raw_payload[..payload_len].chunks_exact(4).enumerate() {
                payload[idx] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            }
        }

        let mut crc = [0u8; 4];
        self.read_fifo_bytes(&mut crc)?;

        Ok(Some(Message::new(header, payload)))
    }

    pub fn send_message(&mut self, message: &Message) -> Result<(), Error> {
        let payload_len = message.object_count() * 4;
        let packed_len = 2 + payload_len;
        if packed_len > 30 {
            return Err(Error::Protocol("tx_payload_too_large"));
        }

        let mut frame = [0u8; FIFO_TX_FRAME_MAX];
        let mut len = 0usize;
        frame[len] = TOKEN_SOP1;
        len += 1;
        frame[len] = TOKEN_SOP1;
        len += 1;
        frame[len] = TOKEN_SOP1;
        len += 1;
        frame[len] = TOKEN_SOP2;
        len += 1;
        frame[len] = TOKEN_PACKSYM | (packed_len as u8);
        len += 1;

        let header_bytes = message.header.raw().to_le_bytes();
        frame[len..len + 2].copy_from_slice(&header_bytes);
        len += 2;

        for object in message.payload() {
            let bytes = object.to_le_bytes();
            frame[len..len + 4].copy_from_slice(&bytes);
            len += 4;
        }

        frame[len] = TOKEN_JAM_CRC;
        len += 1;
        frame[len] = TOKEN_EOP;
        len += 1;
        frame[len] = TOKEN_TX_OFF;
        len += 1;
        frame[len] = TOKEN_TX_ON;
        len += 1;

        self.write_fifo_bytes(&frame[..len])
    }

    pub fn send_hard_reset(&mut self) -> Result<(), Error> {
        let frame = [
            TOKEN_RESET1,
            TOKEN_RESET1,
            TOKEN_RESET1,
            TOKEN_RESET2,
            TOKEN_TX_ON,
        ];
        self.write_fifo_bytes(&frame)
    }

    pub fn release_i2c(self) -> I2C {
        self.i2c
    }

    fn read_reg(&mut self, reg: u8) -> Result<u8, esp_hal::i2c::master::Error> {
        let mut buf = [0u8; 1];
        self.i2c.write_read(I2C_ADDRESS, &[reg], &mut buf)?;
        Ok(buf[0])
    }

    fn read_block(&mut self, reg: u8, buf: &mut [u8]) -> Result<(), esp_hal::i2c::master::Error> {
        self.i2c.write_read(I2C_ADDRESS, &[reg], buf)
    }

    fn write_reg(&mut self, reg: u8, value: u8) -> Result<(), esp_hal::i2c::master::Error> {
        self.i2c.write(I2C_ADDRESS, &[reg, value])
    }

    fn read_fifo_byte(&mut self) -> Result<u8, Error> {
        Ok(self.read_reg(reg::FIFOS)?)
    }

    fn read_fifo_bytes(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        self.read_block(reg::FIFOS, buf)?;
        Ok(())
    }

    fn write_fifo_bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let mut buf = [0u8; FIFO_TX_FRAME_MAX + 1];
        buf[0] = reg::FIFOS;
        buf[1..1 + bytes.len()].copy_from_slice(bytes);
        self.i2c.write(I2C_ADDRESS, &buf[..1 + bytes.len()])?;
        Ok(())
    }
}
