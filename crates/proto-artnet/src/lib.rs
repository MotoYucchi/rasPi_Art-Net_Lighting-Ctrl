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

/// Configuration for building an ArtPollReply packet
#[derive(Debug, Clone)]
pub struct ArtPollReplyConfig<'a> {
    pub ip: [u8; 4],
    pub port: u16,
    pub vers_info: u16,
    pub net_switch: u8,
    pub sub_switch: u8,
    pub oem: u16,
    pub status1: u8,
    pub esta_man: u16,
    pub short_name: &'a str,
    pub long_name: &'a str,
    pub node_report: &'a str,
    pub num_ports: u16,
    pub port_types: [u8; 4],
    pub good_input: [u8; 4],
    pub good_output: [u8; 4],
    pub sw_in: [u8; 4],
    pub sw_out: [u8; 4],
    pub mac: [u8; 6],
    pub bind_ip: [u8; 4],
}

impl<'a> Default for ArtPollReplyConfig<'a> {
    fn default() -> Self {
        Self {
            ip: [0, 0, 0, 0],
            port: ARTNET_PORT,
            vers_info: 0x0100,
            net_switch: 0,
            sub_switch: 0,
            oem: 0x00FF,
            status1: 0xD0,
            esta_man: 0x7FF0,
            short_name: "PixelNode",
            long_name: "Raspberry Pi Art-Net WS2811 Pixel Controller",
            node_report: "#0001 [0000] OK - Ready",
            num_ports: 1,
            port_types: [0x80, 0, 0, 0], // Port 0 is DMX512 output
            good_input: [0; 4],
            good_output: [0x80, 0, 0, 0], // Data being output
            sw_in: [0; 4],
            sw_out: [0, 0, 0, 0],
            mac: [0; 6],
            bind_ip: [0, 0, 0, 0],
        }
    }
}

/// Minimum size of an ArtPollReply packet in bytes
pub const ART_POLL_REPLY_LEN: usize = 239;

/// Serializes an ArtPollReply packet into `buf`.
/// Returns number of bytes written (239).
pub fn write_art_poll_reply(
    buf: &mut [u8],
    cfg: &ArtPollReplyConfig<'_>,
) -> Result<usize, ()> {
    if buf.len() < ART_POLL_REPLY_LEN {
        return Err(());
    }

    buf[..ART_POLL_REPLY_LEN].fill(0);

    // 0..8: Art-Net header
    buf[0..8].copy_from_slice(ARTNET_HEADER.as_slice());
    // 8..10: OpCode PollReply (0x2100 in little-endian -> 0x00, 0x21)
    buf[8..10].copy_from_slice(&u16::to_le_bytes(u16::from(OpCode::PollReply)));
    // 10..14: IP Address
    buf[10..14].copy_from_slice(&cfg.ip);
    // 14..16: Port (little endian)
    buf[14..16].copy_from_slice(&u16::to_le_bytes(cfg.port));
    // 16..18: Version (big-endian)
    buf[16..18].copy_from_slice(&cfg.vers_info.to_be_bytes());
    // 18: NetSwitch
    buf[18] = cfg.net_switch;
    // 19: SubSwitch
    buf[19] = cfg.sub_switch;
    // 20..22: Oem (big-endian)
    buf[20..22].copy_from_slice(&cfg.oem.to_be_bytes());
    // 22: UbeaVersion
    buf[22] = 0;
    // 23: Status1
    buf[23] = cfg.status1;
    // 24..26: EstaMan (little-endian)
    buf[24..26].copy_from_slice(&cfg.esta_man.to_le_bytes());

    // 26..44: ShortName (max 17 chars + null)
    let s_bytes = cfg.short_name.as_bytes();
    let s_len = s_bytes.len().min(17);
    buf[26..26 + s_len].copy_from_slice(&s_bytes[..s_len]);

    // 44..108: LongName (max 63 chars + null)
    let l_bytes = cfg.long_name.as_bytes();
    let l_len = l_bytes.len().min(63);
    buf[44..44 + l_len].copy_from_slice(&l_bytes[..l_len]);

    // 108..172: NodeReport (max 63 chars + null)
    let r_bytes = cfg.node_report.as_bytes();
    let r_len = r_bytes.len().min(63);
    buf[108..108 + r_len].copy_from_slice(&r_bytes[..r_len]);

    // 172..174: NumPorts (Hi, Lo)
    buf[172..174].copy_from_slice(&cfg.num_ports.to_be_bytes());

    // 174..178: PortTypes
    buf[174..178].copy_from_slice(&cfg.port_types);
    // 178..182: GoodInput
    buf[178..182].copy_from_slice(&cfg.good_input);
    // 182..186: GoodOutput
    buf[182..186].copy_from_slice(&cfg.good_output);
    // 186..190: SwIn
    buf[186..190].copy_from_slice(&cfg.sw_in);
    // 190..194: SwOut
    buf[190..194].copy_from_slice(&cfg.sw_out);

    // 200: Style (0x00 = StNode)
    buf[200] = 0x00;
    // 201..207: MAC
    buf[201..207].copy_from_slice(&cfg.mac);
    // 207..211: BindIp
    buf[207..211].copy_from_slice(&cfg.bind_ip);
    // 211: BindIndex
    buf[211] = 1;
    // 212: Status2 (0x08 = DHCP & Art-Net 3/4)
    buf[212] = 0x08;

    Ok(ART_POLL_REPLY_LEN)
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

    #[test]
    fn test_write_art_poll_reply() {
        let mut buf = [0u8; 300];
        let mut cfg = ArtPollReplyConfig::default();
        cfg.ip = [192, 168, 1, 100];
        cfg.short_name = "StagePi";
        let len = write_art_poll_reply(&mut buf, &cfg).unwrap();
        assert_eq!(len, ART_POLL_REPLY_LEN);
        assert_eq!(&buf[0..8], ARTNET_HEADER);
        // OpCode 0x2100 in LE: [0x00, 0x21]
        assert_eq!(&buf[8..10], &[0x00, 0x21]);
        assert_eq!(&buf[10..14], &[192, 168, 1, 100]);
        assert_eq!(&buf[26..33], b"StagePi");
    }
}
