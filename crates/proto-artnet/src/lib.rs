//! Art-Net 4 zero-allocation packet parser and serializer.
//!
//! Compliant with Art-Net 4 specification.
//! Designed for real-time safety: no allocations, no panics, bounds-checked.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

/// Art-Net protocol header signature: "Art-Net\0"
pub const ARTNET_HEADER: &[u8; 8] = b"Art-Net\0";

/// Art-Net Protocol Version 14
pub const PROTOCOL_VERSION: u16 = 14;

/// Standard Art-Net UDP Port (0x1936)
pub const ARTNET_PORT: u16 = 6454;

/// Art-Net Operation Codes (16-bit, little-endian encoded on wire)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum OpCode {
    Poll = 0x2000,
    PollReply = 0x2100,
    Dmx = 0x5000,
    Sync = 0x5200,
    Address = 0x6000,
    Input = 0x7000,
    TodRequest = 0x8000,
    TodData = 0x8100,
    TodControl = 0x8200,
    Rdm = 0x8300,
    IpProg = 0xF800,
    IpProgReply = 0xF900,
    Unknown(u16),
}

impl From<u16> for OpCode {
    fn from(val: u16) -> Self {
        match val {
            0x2000 => OpCode::Poll,
            0x2100 => OpCode::PollReply,
            0x5000 => OpCode::Dmx,
            0x5200 => OpCode::Sync,
            0x6000 => OpCode::Address,
            0x7000 => OpCode::Input,
            0x8000 => OpCode::TodRequest,
            0x8100 => OpCode::TodData,
            0x8200 => OpCode::TodControl,
            0x8300 => OpCode::Rdm,
            0xF800 => OpCode::IpProg,
            0xF900 => OpCode::IpProgReply,
            other => OpCode::Unknown(other),
        }
    }
}

impl From<OpCode> for u16 {
    fn from(op: OpCode) -> Self {
        match op {
            OpCode::Poll => 0x2000,
            OpCode::PollReply => 0x2100,
            OpCode::Dmx => 0x5000,
            OpCode::Sync => 0x5200,
            OpCode::Address => 0x6000,
            OpCode::Input => 0x7000,
            OpCode::TodRequest => 0x8000,
            OpCode::TodData => 0x8100,
            OpCode::TodControl => 0x8200,
            OpCode::Rdm => 0x8300,
            OpCode::IpProg => 0xF800,
            OpCode::IpProgReply => 0xF900,
            OpCode::Unknown(val) => val,
        }
    }
}

/// Errors occurring during Art-Net parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    TooShort,
    InvalidHeader,
    InvalidOpCode,
    UnsupportedVersion(u16),
    InvalidLength,
}

/// Parsed Art-Net packet (borrowed, zero-copy)
#[derive(Debug, PartialEq, Eq)]
pub enum ArtNetPacket<'a> {
    Dmx(ArtDmx<'a>),
    Poll(ArtPoll),
    Sync,
    Other(OpCode),
}

/// ArtDmx packet view (zero-allocation)
#[derive(Debug, PartialEq, Eq)]
pub struct ArtDmx<'a> {
    pub sequence: u8,
    pub physical: u8,
    /// 15-bit Port-Address: (Net << 8) | (SubNet << 4) | Universe
    pub port_address: u16,
    pub data: &'a [u8],
}

impl<'a> ArtDmx<'a> {
    /// Extracts 7-bit Net (0..127)
    #[inline]
    pub fn net(&self) -> u8 {
        ((self.port_address >> 8) & 0x7F) as u8
    }

    /// Extracts 4-bit SubNet (0..15)
    #[inline]
    pub fn sub_net(&self) -> u8 {
        ((self.port_address >> 4) & 0x0F) as u8
    }

    /// Extracts 4-bit Universe (0..15)
    #[inline]
    pub fn universe(&self) -> u8 {
        (self.port_address & 0x0F) as u8
    }
}

/// ArtPoll packet
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtPoll {
    pub talk_to_me: u8,
    pub priority: u8,
}

/// Parse raw UDP datagram into an `ArtNetPacket`
pub fn parse_artnet(buf: &[u8]) -> Result<ArtNetPacket<'_>, ParseError> {
    if buf.len() < 10 {
        return Err(ParseError::TooShort);
    }

    if buf.get(0..8) != Some(ARTNET_HEADER.as_slice()) {
        return Err(ParseError::InvalidHeader);
    }

    let op_raw = u16::from_le_bytes([
        *buf.get(8).ok_or(ParseError::TooShort)?,
        *buf.get(9).ok_or(ParseError::TooShort)?,
    ]);
    let op = OpCode::from(op_raw);

    match op {
        OpCode::Dmx => {
            // ArtDmx minimum header is 18 bytes
            if buf.len() < 18 {
                return Err(ParseError::TooShort);
            }
            let prot_ver = u16::from_be_bytes([
                *buf.get(10).ok_or(ParseError::TooShort)?,
                *buf.get(11).ok_or(ParseError::TooShort)?,
            ]);
            if prot_ver < PROTOCOL_VERSION {
                return Err(ParseError::UnsupportedVersion(prot_ver));
            }

            let sequence = *buf.get(12).ok_or(ParseError::TooShort)?;
            let physical = *buf.get(13).ok_or(ParseError::TooShort)?;
            let sub_uni = *buf.get(14).ok_or(ParseError::TooShort)?;
            let net = *buf.get(15).ok_or(ParseError::TooShort)?;
            let port_address = ((net as u16 & 0x7F) << 8) | (sub_uni as u16);

            let length = u16::from_be_bytes([
                *buf.get(16).ok_or(ParseError::TooShort)?,
                *buf.get(17).ok_or(ParseError::TooShort)?,
            ]) as usize;

            if length < 2 || length > 512 {
                return Err(ParseError::InvalidLength);
            }

            let payload_available = buf.len().saturating_sub(18);
            if payload_available < length {
                return Err(ParseError::InvalidLength);
            }

            let dmx_slice = buf
                .get(18..18 + length)
                .ok_or(ParseError::InvalidLength)?;

            Ok(ArtNetPacket::Dmx(ArtDmx {
                sequence,
                physical,
                port_address,
                data: dmx_slice,
            }))
        }
        OpCode::Poll => {
            if buf.len() < 12 {
                return Err(ParseError::TooShort);
            }
            let talk_to_me = *buf.get(12).unwrap_or(&0);
            let priority = *buf.get(13).unwrap_or(&0);
            Ok(ArtNetPacket::Poll(ArtPoll {
                talk_to_me,
                priority,
            }))
        }
        OpCode::Sync => Ok(ArtNetPacket::Sync),
        other => Ok(ArtNetPacket::Other(other)),
    }
}

/// Helper to serialize an ArtDmx packet into a provided buffer without heap allocation.
/// Returns number of bytes written.
pub fn write_art_dmx(
    buf: &mut [u8],
    sequence: u8,
    physical: u8,
    port_address: u16,
    dmx_data: &[u8],
) -> Result<usize, ()> {
    let len = dmx_data.len();
    if len < 2 || len > 512 {
        return Err(());
    }
    let total_len = 18 + len;
    if buf.len() < total_len {
        return Err(());
    }

    buf[0..8].copy_from_slice(ARTNET_HEADER.as_slice());
    buf[8..10].copy_from_slice(&u16::to_le_bytes(u16::from(OpCode::Dmx)));
    buf[10..12].copy_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    buf[12] = sequence;
    buf[13] = physical;
    buf[14] = (port_address & 0xFF) as u8;
    buf[15] = ((port_address >> 8) & 0x7F) as u8;
    buf[16..18].copy_from_slice(&(len as u16).to_be_bytes());
    buf[18..total_len].copy_from_slice(dmx_data);

    Ok(total_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_art_dmx_roundtrip() {
        let mut buf = [0u8; 128];
        let dmx_payload = [10u8, 20, 30, 40, 50, 60, 70, 80];
        let written = write_art_dmx(&mut buf, 42, 1, 0x0105, &dmx_payload).unwrap();

        let parsed = parse_artnet(&buf[..written]).unwrap();
        match parsed {
            ArtNetPacket::Dmx(dmx) => {
                assert_eq!(dmx.sequence, 42);
                assert_eq!(dmx.physical, 1);
                assert_eq!(dmx.port_address, 0x0105);
                assert_eq!(dmx.net(), 1);
                assert_eq!(dmx.sub_net(), 0);
                assert_eq!(dmx.universe(), 5);
                assert_eq!(dmx.data, &dmx_payload);
            }
            _ => panic!("Expected ArtDmx packet"),
        }
    }

    #[test]
    fn test_invalid_header() {
        let garbage = [0u8; 32];
        assert_eq!(parse_artnet(&garbage), Err(ParseError::InvalidHeader));
    }
}
