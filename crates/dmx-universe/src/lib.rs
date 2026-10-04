//! DMX Universe management, source tracking, priority merging, and failsafe timeout detection.
//!
//! Designed for real-time safety: no allocations during updates, bounds-checked.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::time::Duration;
use std::time::Instant;

pub const DMX_UNIVERSE_SIZE: usize = 512;
pub const MAX_SUBSCRIBED_UNIVERSES: usize = 32;
pub const SACN_DEFAULT_TIMEOUT: Duration = Duration::from_millis(2500);

/// 512-byte DMX channel buffer
pub type DmxChannels = [u8; DMX_UNIVERSE_SIZE];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolSource {
    None,
    ArtNet,
    Sacn { priority: u8 },
}

/// State of a single subscribed universe
#[derive(Debug, Clone)]
pub struct UniverseState {
    pub universe_id: u16,
    pub channels: DmxChannels,
    pub last_seen: Option<Instant>,
    pub source: ProtocolSource,
    pub active: bool,
}

impl UniverseState {
    pub fn new(universe_id: u16) -> Self {
        Self {
            universe_id,
            channels: [0u8; DMX_UNIVERSE_SIZE],
            last_seen: None,
            source: ProtocolSource::None,
            active: false,
        }
    }

    /// Read DMX channel value (1-indexed, 1..=512)
    #[inline]
    pub fn get_channel(&self, ch: u16) -> u8 {
        if ch >= 1 && ch <= 512 {
            self.channels[(ch - 1) as usize]
        } else {
            0
        }
    }
}

/// Fail-safe policy when DMX signal is lost
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailsafeMode {
    /// Keep last received values indefinitely (Recommended for live shows)
    Hold,
    /// Hold for specified duration, then fade out to blackout
    HoldThenFade { hold_duration: Duration, fade_duration: Duration },
    /// Fade to a predefined safe scene
    FallbackScene { hold_duration: Duration },
    /// Immediate blackout upon timeout
    Blackout,
}

/// Universe storage supporting up to MAX_SUBSCRIBED_UNIVERSES with zero runtime allocation
#[derive(Debug)]
pub struct UniverseStore {
    states: [Option<UniverseState>; MAX_SUBSCRIBED_UNIVERSES],
    count: usize,
}

impl UniverseStore {
    pub fn new() -> Self {
        Self {
            states: Default::default(),
            count: 0,
        }
    }

    /// Register a universe to subscribe to.
    pub fn subscribe(&mut self, universe_id: u16) -> bool {
        for state_opt in self.states.iter() {
            if let Some(state) = state_opt {
                if state.universe_id == universe_id {
                    return true; // Already subscribed
                }
            }
        }
        if self.count >= MAX_SUBSCRIBED_UNIVERSES {
            return false;
        }
        for state_opt in self.states.iter_mut() {
            if state_opt.is_none() {
                *state_opt = Some(UniverseState::new(universe_id));
                self.count += 1;
                return true;
            }
        }
        false
    }

    /// Update universe from Art-Net packet
    pub fn update_artnet(&mut self, universe_id: u16, data: &[u8], now: Instant) {
        for state_opt in self.states.iter_mut() {
            if let Some(state) = state_opt {
                if state.universe_id == universe_id {
                    // If sACN is actively providing data, ignore lower priority Art-Net
                    if let ProtocolSource::Sacn { .. } = state.source {
                        if state.active {
                            return;
                        }
                    }

                    let copy_len = data.len().min(DMX_UNIVERSE_SIZE);
                    state.channels[..copy_len].copy_from_slice(&data[..copy_len]);
                    state.last_seen = Some(now);
                    state.source = ProtocolSource::ArtNet;
                    state.active = true;
                    return;
                }
            }
        }
    }

    /// Update universe from sACN packet
    pub fn update_sacn(
        &mut self,
        universe_id: u16,
        priority: u8,
        data: &[u8],
        is_terminated: bool,
        now: Instant,
    ) {
        for state_opt in self.states.iter_mut() {
            if let Some(state) = state_opt {
                if state.universe_id == universe_id {
                    if is_terminated {
                        state.active = false;
                        return;
                    }

                    // Priority check
                    if let ProtocolSource::Sacn { priority: current_pri } = state.source {
                        if state.active && priority < current_pri {
                            return; // Lower priority ignored
                        }
                    }

                    let copy_len = data.len().min(DMX_UNIVERSE_SIZE);
                    state.channels[..copy_len].copy_from_slice(&data[..copy_len]);
                    state.last_seen = Some(now);
                    state.source = ProtocolSource::Sacn { priority };
                    state.active = true;
                    return;
                }
            }
        }
    }

    /// Get universe state by ID
    pub fn get(&self, universe_id: u16) -> Option<&UniverseState> {
        for state_opt in self.states.iter() {
            if let Some(state) = state_opt {
                if state.universe_id == universe_id {
                    return Some(state);
                }
            }
        }
        None
    }

    /// Check timeouts and mark inactive universes
    pub fn check_timeouts(&mut self, now: Instant, timeout: Duration) {
        for state_opt in self.states.iter_mut() {
            if let Some(state) = state_opt {
                if state.active {
                    if let Some(last) = state.last_seen {
                        if now.saturating_duration_since(last) > timeout {
                            state.active = false;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_universe_subscription_and_update() {
        let mut store = UniverseStore::new();
        assert!(store.subscribe(1));
        assert!(store.subscribe(2));

        let now = Instant::now();
        let payload = [100, 200, 255];
        store.update_artnet(1, &payload, now);

        let u1 = store.get(1).unwrap();
        assert!(u1.active);
        assert_eq!(u1.get_channel(1), 100);
        assert_eq!(u1.get_channel(2), 200);
        assert_eq!(u1.get_channel(3), 255);
        assert_eq!(u1.get_channel(4), 0);

        // sACN higher priority overrides ArtNet
        let sacn_payload = [10, 20, 30];
        store.update_sacn(1, 150, &sacn_payload, false, now);
        let u1_after = store.get(1).unwrap();
        assert_eq!(u1_after.get_channel(1), 10);
    }
}
