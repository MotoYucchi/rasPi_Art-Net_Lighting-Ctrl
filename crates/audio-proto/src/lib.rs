//! Lightweight 64-byte Audio Feature Packet protocol for live lighting sync.
//!
//! Sent by host audio analyzer via UDP broadcast/unicast at 60-100Hz.
//! Bounds-checked, zero-alloc, panic-free.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub const AUDIO_MAGIC: [u8; 4] = *b"PXAU";
pub const AUDIO_PROTO_VERSION: u8 = 1;
pub const AUDIO_PACKET_LEN: usize = 64;

pub const TRIGGER_BEAT: u8 = 0x01;
pub const TRIGGER_KICK: u8 = 0x02;
pub const TRIGGER_SNARE: u8 = 0x04;
pub const TRIGGER_HIHAT: u8 = 0x08;

/// Fixed-size 64-byte audio feature frame
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFeaturePacket {
    pub sequence: u32,
    pub timestamp_us: u64,
    /// 7-band normalized energy (0..255)
    /// 0: Sub-Bass (30-60Hz)
    /// 1: Bass (60-140Hz)
    /// 2: Low-Mid (140-400Hz)
    /// 3: Mid (400-1.5kHz)
    /// 4: High-Mid (1.5k-4kHz)
    /// 5: Presence (4k-8kHz)
    /// 6: Brilliance (8k-16kHz)
    pub bands: [u8; 7],
    pub rms_level: u8,
    pub peak_level: u8,
    pub spectral_centroid: u8,
    pub triggers: u8,
    /// BPM * 100 (e.g. 12850 = 128.5 BPM)
    pub bpm: u16,
    /// Beat Phase (0..255, 0 = on beat, 128 = off-beat)
    pub beat_phase: u8,
}

impl Default for AudioFeaturePacket {
    fn default() -> Self {
        Self {
            sequence: 0,
            timestamp_us: 0,
            bands: [0; 7],
            rms_level: 0,
            peak_level: 0,
            spectral_centroid: 128,
            triggers: 0,
            bpm: 12000,
            beat_phase: 0,
        }
    }
}

impl AudioFeaturePacket {
    #[inline]
    pub fn is_beat(&self) -> bool {
        (self.triggers & TRIGGER_BEAT) != 0
    }

    #[inline]
    pub fn is_kick(&self) -> bool {
        (self.triggers & TRIGGER_KICK) != 0
    }

    #[inline]
    pub fn is_snare(&self) -> bool {
        (self.triggers & TRIGGER_SNARE) != 0
    }

    #[inline]
    pub fn is_hihat(&self) -> bool {
        (self.triggers & TRIGGER_HIHAT) != 0
    }

    /// Serialize into a 64-byte array
    pub fn serialize(&self) -> [u8; AUDIO_PACKET_LEN] {
        let mut buf = [0u8; AUDIO_PACKET_LEN];
        buf[0..4].copy_from_slice(&AUDIO_MAGIC);
        buf[4] = AUDIO_PROTO_VERSION;
        buf[5..9].copy_from_slice(&self.sequence.to_be_bytes());
        buf[9..17].copy_from_slice(&self.timestamp_us.to_be_bytes());
        buf[17..24].copy_from_slice(&self.bands);
        buf[24] = self.rms_level;
        buf[25] = self.peak_level;
        buf[26] = self.spectral_centroid;
        buf[27] = self.triggers;
        buf[28..30].copy_from_slice(&self.bpm.to_be_bytes());
        buf[30] = self.beat_phase;
        // 31..60 is reserved (zeros)

        let crc = crc32fast::hash(&buf[0..60]);
        buf[60..64].copy_from_slice(&crc.to_be_bytes());
        buf
    }

    /// Parse and validate a 64-byte slice
    pub fn parse(buf: &[u8]) -> Result<Self, &'static str> {
        if buf.len() < AUDIO_PACKET_LEN {
            return Err("Packet too short");
        }
        if buf.get(0..4) != Some(&AUDIO_MAGIC) {
            return Err("Invalid magic");
        }
        if buf[4] != AUDIO_PROTO_VERSION {
            return Err("Unsupported version");
        }

        let expected_crc = u32::from_be_bytes([buf[60], buf[61], buf[62], buf[63]]);
        let actual_crc = crc32fast::hash(&buf[0..60]);
        if expected_crc != actual_crc {
            return Err("CRC32 mismatch");
        }

        let sequence = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]);
        let timestamp_us = u64::from_be_bytes([
            buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15], buf[16],
        ]);
        let mut bands = [0u8; 7];
        bands.copy_from_slice(&buf[17..24]);
        let rms_level = buf[24];
        let peak_level = buf[25];
        let spectral_centroid = buf[26];
        let triggers = buf[27];
        let bpm = u16::from_be_bytes([buf[28], buf[29]]);
        let beat_phase = buf[30];

        Ok(Self {
            sequence,
            timestamp_us,
            bands,
            rms_level,
            peak_level,
            spectral_centroid,
            triggers,
            bpm,
            beat_phase,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_packet_roundtrip() {
        let pkt = AudioFeaturePacket {
            sequence: 1234,
            timestamp_us: 987654321,
            bands: [10, 20, 30, 40, 50, 60, 70],
            rms_level: 180,
            peak_level: 240,
            spectral_centroid: 150,
            triggers: TRIGGER_BEAT | TRIGGER_KICK,
            bpm: 13000,
            beat_phase: 64,
        };

        let raw = pkt.serialize();
        assert_eq!(raw.len(), 64);

        let parsed = AudioFeaturePacket::parse(&raw).unwrap();
        assert_eq!(pkt, parsed);
        assert!(parsed.is_beat());
        assert!(parsed.is_kick());
        assert!(!parsed.is_snare());
    }

    #[test]
    fn test_corrupt_packet_detected() {
        let pkt = AudioFeaturePacket::default();
        let mut raw = pkt.serialize();
        raw[20] ^= 0xFF; // corrupt band data

        assert!(AudioFeaturePacket::parse(&raw).is_err());
    }
}
