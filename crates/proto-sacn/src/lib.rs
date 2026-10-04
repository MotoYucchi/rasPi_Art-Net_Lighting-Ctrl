//! sACN (ANSI E1.31-2018) zero-allocation packet parser and serializer.
//!
//! Compliant with ANSI E1.31-2018.
//! Real-time safe: no dynamic memory allocations, no panics.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

/// Standard sACN UDP Port
pub const SACN_PORT: u16 = 5568;

/// ACN Packet Identifier: "ASC-E1.17\0\0\0" (12 bytes)
pub const ACN_PACKET_IDENTIFIER: [u8; 12] = [
    0x41, 0x53, 0x43, 0x2d, 0x45, 0x31, 0x2e, 0x31, 0x37, 0x00, 0x00, 0x00,
];

/// Root layer vector for E1.31 data packet
pub const VECTOR_ROOT_E131_DATA: u32 = 0x00000004;
/// Root layer vector for E1.31 extended packet (e.g. Synchronization)
pub const VECTOR_ROOT_E131_EXTENDED: u32 = 0x00000008;

/// Framing layer vector for DMP set property
pub const VECTOR_E131_DATA_PACKET: u32 = 0x00000002;
/// Framing layer vector for Universe Synchronization
pub const VECTOR_E131_EXTENDED_SYNCHRONIZATION: u32 = 0x00000001;

/// DMP layer vector for set property
pub const VECTOR_DMP_SET_PROPERTY: u8 = 0x02;

/// sACN Framing Layer Options flags
pub const OPTION_TERMINATED_DATA: u8 = 0x40;
pub const OPTION_FORCE_SYNCHRONIZATION: u8 = 0x20;
pub const OPTION_PREVIEW_DATA: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SacnParseError {
    TooShort,
    InvalidPreamble,
    InvalidPostamble,
    InvalidAcnIdentifier,
    UnsupportedRootVector(u32),
    UnsupportedFramingVector(u32),
    UnsupportedDmpVector(u8),
    InvalidLength,
    InvalidStartCode(u8),
}

/// Borrowed, zero-copy sACN Data Packet view
#[derive(Debug, PartialEq, Eq)]
pub struct SacmData<'a> {
    pub cid: [u8; 16],
    pub source_name: &'a str,
    pub priority: u8,
    pub sync_universe: u16,
    pub sequence: u8,
    pub options: u8,
    pub universe: u16,
    pub start_code: u8,
    pub data: &'a [u8],
}

impl<'a> SacmData<'a> {
    #[inline]
    pub fn is_stream_terminated(&self) -> bool {
        (self.options & OPTION_TERMINATED_DATA) != 0
    }

    #[inline]
    pub fn is_preview(&self) -> bool {
        (self.options & OPTION_PREVIEW_DATA) != 0
    }
}

/// Parsed sACN Packet
#[derive(Debug, PartialEq, Eq)]
pub enum SacnPacket<'a> {
    Data(SacmData<'a>),
    Sync {
        sequence: u8,
        sync_universe: u16,
    },
}

/// Calculate multicast IPv4 address for a given universe (1..63999)
#[inline]
pub fn universe_to_multicast_ip(universe: u16) -> [u8; 4] {
    let hi = ((universe >> 8) & 0xFF) as u8;
    let lo = (universe & 0xFF) as u8;
    [239, 255, hi, lo]
}

/// Parse raw UDP datagram into `SacnPacket`
pub fn parse_sacn(buf: &[u8]) -> Result<SacnPacket<'_>, SacnParseError> {
    if buf.len() < 38 {
        return Err(SacnParseError::TooShort);
    }

    // 1. Root Layer
    let preamble_size = u16::from_be_bytes([*buf.get(0).ok_or(SacnParseError::TooShort)?, *buf.get(1).ok_or(SacnParseError::TooShort)?]);
    if preamble_size != 0x0010 {
        return Err(SacnParseError::InvalidPreamble);
    }
    let postamble_size = u16::from_be_bytes([*buf.get(2).ok_or(SacnParseError::TooShort)?, *buf.get(3).ok_or(SacnParseError::TooShort)?]);
    if postamble_size != 0x0000 {
        return Err(SacnParseError::InvalidPostamble);
    }

    if buf.get(4..16) != Some(&ACN_PACKET_IDENTIFIER) {
        return Err(SacnParseError::InvalidAcnIdentifier);
    }

    let root_vector = u32::from_be_bytes([
        *buf.get(18).ok_or(SacnParseError::TooShort)?,
        *buf.get(19).ok_or(SacnParseError::TooShort)?,
        *buf.get(20).ok_or(SacnParseError::TooShort)?,
        *buf.get(21).ok_or(SacnParseError::TooShort)?,
    ]);

    let mut cid = [0u8; 16];
    let cid_src = buf.get(22..38).ok_or(SacnParseError::TooShort)?;
    cid.copy_from_slice(cid_src);

    if root_vector == VECTOR_ROOT_E131_DATA {
        // 2. Framing Layer (starts at offset 38)
        if buf.len() < 115 {
            return Err(SacnParseError::TooShort);
        }

        let framing_vector = u32::from_be_bytes([
            *buf.get(40).ok_or(SacnParseError::TooShort)?,
            *buf.get(41).ok_or(SacnParseError::TooShort)?,
            *buf.get(42).ok_or(SacnParseError::TooShort)?,
            *buf.get(43).ok_or(SacnParseError::TooShort)?,
        ]);

        if framing_vector != VECTOR_E131_DATA_PACKET {
            return Err(SacnParseError::UnsupportedFramingVector(framing_vector));
        }

        let raw_name = buf.get(44..108).ok_or(SacnParseError::TooShort)?;
        let null_idx = raw_name.iter().position(|&c| c == 0).unwrap_or(raw_name.len());
        let source_name = core::str::from_utf8(&raw_name[..null_idx]).unwrap_or("");

        let priority = *buf.get(108).ok_or(SacnParseError::TooShort)?;
        let sync_universe = u16::from_be_bytes([*buf.get(109).ok_or(SacnParseError::TooShort)?, *buf.get(110).ok_or(SacnParseError::TooShort)?]);
        let sequence = *buf.get(111).ok_or(SacnParseError::TooShort)?;
        let options = *buf.get(112).ok_or(SacnParseError::TooShort)?;
        let universe = u16::from_be_bytes([*buf.get(113).ok_or(SacnParseError::TooShort)?, *buf.get(114).ok_or(SacnParseError::TooShort)?]);

        // 3. DMP Layer (starts at offset 115)
        if buf.len() < 126 {
            return Err(SacnParseError::TooShort);
        }

        let dmp_vector = *buf.get(117).ok_or(SacnParseError::TooShort)?;
        if dmp_vector != VECTOR_DMP_SET_PROPERTY {
            return Err(SacnParseError::UnsupportedDmpVector(dmp_vector));
        }

        let prop_val_count = u16::from_be_bytes([*buf.get(123).ok_or(SacnParseError::TooShort)?, *buf.get(124).ok_or(SacnParseError::TooShort)?]) as usize;
        if prop_val_count == 0 {
            return Err(SacnParseError::InvalidLength);
        }

        let start_code = *buf.get(125).ok_or(SacnParseError::TooShort)?;
        let dmx_count = prop_val_count.saturating_sub(1);
        let end_idx = 126 + dmx_count;

        if buf.len() < end_idx || dmx_count > 512 {
            return Err(SacnParseError::InvalidLength);
        }

        let data = buf.get(126..end_idx).ok_or(SacnParseError::InvalidLength)?;

        Ok(SacnPacket::Data(SacmData {
            cid,
            source_name,
            priority,
            sync_universe,
            sequence,
            options,
            universe,
            start_code,
            data,
        }))
    } else if root_vector == VECTOR_ROOT_E131_EXTENDED {
        // Universe Synchronization packet
        if buf.len() < 49 {
            return Err(SacnParseError::TooShort);
        }
        let sequence = *buf.get(44).ok_or(SacnParseError::TooShort)?;
        let sync_universe = u16::from_be_bytes([*buf.get(45).ok_or(SacnParseError::TooShort)?, *buf.get(46).ok_or(SacnParseError::TooShort)?]);
        Ok(SacnPacket::Sync {
            sequence,
            sync_universe,
        })
    } else {
        Err(SacnParseError::UnsupportedRootVector(root_vector))
    }
}

/// Helper to format and serialize an sACN Data packet into a buffer (e.g. for testing)
pub fn write_sacn_data(
    buf: &mut [u8],
    cid: &[u8; 16],
    source_name: &str,
    priority: u8,
    universe: u16,
    sequence: u8,
    dmx_data: &[u8],
) -> Result<usize, ()> {
    let dmx_len = dmx_data.len();
    if dmx_len > 512 {
        return Err(());
    }
    let total_len = 126 + dmx_len;
    if buf.len() < total_len {
        return Err(());
    }

    // Root Layer
    buf[0..2].copy_from_slice(&0x0010u16.to_be_bytes()); // Preamble
    buf[2..4].copy_from_slice(&0x0000u16.to_be_bytes()); // Postamble
    buf[4..16].copy_from_slice(&ACN_PACKET_IDENTIFIER);
    let root_fl = 0x7000u16 | ((total_len - 16) as u16);
    buf[16..18].copy_from_slice(&root_fl.to_be_bytes());
    buf[18..22].copy_from_slice(&VECTOR_ROOT_E131_DATA.to_be_bytes());
    buf[22..38].copy_from_slice(cid);

    // Framing Layer
    let framing_fl = 0x7000u16 | ((total_len - 38) as u16);
    buf[38..40].copy_from_slice(&framing_fl.to_be_bytes());
    buf[40..44].copy_from_slice(&VECTOR_E131_DATA_PACKET.to_be_bytes());
    // Source name (64 bytes, null-padded)
    buf[44..108].fill(0);
    let name_bytes = source_name.as_bytes();
    let name_len = name_bytes.len().min(63);
    buf[44..44 + name_len].copy_from_slice(&name_bytes[..name_len]);
    buf[108] = priority;
    buf[109..111].copy_from_slice(&0u16.to_be_bytes()); // sync universe
    buf[111] = sequence;
    buf[112] = 0; // options
    buf[113..115].copy_from_slice(&universe.to_be_bytes());

    // DMP Layer
    let dmp_fl = 0x7000u16 | ((total_len - 115) as u16);
    buf[115..117].copy_from_slice(&dmp_fl.to_be_bytes());
    buf[117] = VECTOR_DMP_SET_PROPERTY;
    buf[118] = 0xa1; // Address type & data type
    buf[119..121].copy_from_slice(&0x0000u16.to_be_bytes()); // First prop addr
    buf[121..123].copy_from_slice(&0x0001u16.to_be_bytes()); // Address increment
    let prop_val_count = (1 + dmx_len) as u16;
    buf[123..125].copy_from_slice(&prop_val_count.to_be_bytes());
    buf[125] = 0x00; // DMX Start Code
    buf[126..total_len].copy_from_slice(dmx_data);

    Ok(total_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sacn_data_roundtrip() {
        let mut buf = [0u8; 256];
        let cid = [0xAB; 16];
        let dmx = [255, 128, 64, 32, 16, 8, 4, 2];
        let written = write_sacn_data(&mut buf, &cid, "Console1", 100, 1, 42, &dmx).unwrap();

        let parsed = parse_sacn(&buf[..written]).unwrap();
        match parsed {
            SacnPacket::Data(data) => {
                assert_eq!(data.cid, cid);
                assert_eq!(data.source_name, "Console1");
                assert_eq!(data.priority, 100);
                assert_eq!(data.universe, 1);
                assert_eq!(data.sequence, 42);
                assert_eq!(data.start_code, 0x00);
                assert_eq!(data.data, &dmx);
            }
            _ => panic!("Expected SacnPacket::Data"),
        }
    }

    #[test]
    fn test_universe_to_multicast() {
        assert_eq!(universe_to_multicast_ip(1), [239, 255, 0, 1]);
        assert_eq!(universe_to_multicast_ip(258), [239, 255, 1, 2]);
    }
}
