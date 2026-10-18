//! LED strip hardware profiles and preset configurations.
//!
//! Provides specifications for common addressable LED strips, power requirements,
//! voltage drop calculations, color orders, and configuration generators.

use serde::{Deserialize, Serialize};

/// Specification for an addressable LED tape/strip product.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TapeProfile {
    /// Unique identifier for CLI and configuration matching
    pub id: &'static str,
    /// Common alternative identifiers and shortcut aliases
    pub aliases: &'static [&'static str],
    /// Human-readable model / brand display name
    pub name: &'static str,
    /// Detailed description of strip type and characteristics
    pub description: &'static str,
    /// Operating DC voltage (e.g. 24, 12, 5)
    pub voltage: u32,
    /// Driver chip protocol (e.g. "ws2811", "ws2812b", "sk6812")
    pub chip: &'static str,
    /// Color channel order as wired on the physical strip ("rgb", "rbg", "grb", "brg", etc.)
    pub color_order: &'static str,
    /// Physical LED package density per meter
    pub leds_per_meter: u32,
    /// Number of physical LEDs grouped per WS2811 IC / addressable pixel
    pub leds_per_ic: u32,
    /// Controllable pixels (ICs) per meter
    pub pixels_per_meter: u32,
    /// Standard reel or installation length in meters
    pub total_length_m: f32,
    /// Total addressable pixels across the specified length
    pub total_pixels: usize,
    /// Total physical LED emitters across the specified length
    pub total_physical_leds: usize,
    /// Recommended Automatic Brightness Limiter (ABL) current cap in mA
    pub recommended_psu_ma: u32,
    /// Practical field guidance for power injection to avoid voltage drop
    pub power_injection_notes: &'static str,
    /// Practical field guidance for signal wiring, level shifting, and grounding
    pub wiring_notes: &'static str,
}

/// Catalog of pre-configured LED strip profiles.
pub static BUILTIN_PROFILES: &[TapeProfile] = &[
    TapeProfile {
        id: "btf_fcob_576_24v_10m",
        aliases: &[
            "btf",
            "btf-fcob",
            "btf_fcob",
            "btf-fcob-24v",
            "btf-fcob-24v-10m",
            "fcob-24v",
            "1",
        ],
        name: "BTF-LIGHTING FCOB 576 LEDs/m 24V (10m)",
        description: "High-density dotless COB tape, 36 LEDs/WS2811 IC (16 px/m), 160 pixels total, RGB color order",
        voltage: 24,
        chip: "ws2811",
        color_order: "rgb",
        leds_per_meter: 576,
        leds_per_ic: 36,
        pixels_per_meter: 16,
        total_length_m: 10.0,
        total_pixels: 160,
        total_physical_leds: 5760,
        recommended_psu_ma: 7000,
        power_injection_notes: "24V drops significantly less than 5V/12V, but across a 10m run at full white (~150W / ~6.25A), voltage drop will warm traces and cause dimming towards the center. Inject 24V and GND at both ends (0m and 10m). For sustained 100% white show scenes, also inject at the 5m midpoint.",
        wiring_notes: "Connect Common GND between 24V PSU DC negative and Raspberry Pi / Arduino GND. Logic level on DI pin requires 5V logic: use a 74AHCT125 buffer on Pi 3.3V GPIO10, or a 330 ohm series damping resistor on Arduino Pin D6. Never connect 24V to controller GPIO.",
    },
    TapeProfile {
        id: "nobrand_60led_12v_5m",
        aliases: &[
            "generic",
            "nobrand",
            "nobrand-12v",
            "generic-12v",
            "nobrand-12v-5m",
            "generic-12v-5m",
            "nobrand_12v_5m",
            "generic_12v_5m",
            "2",
        ],
        name: "Generic / No-brand 60 LEDs/m 12V (5m)",
        description: "Standard 5050 SMD tape, 3 LEDs/WS2811 IC (20 px/m), 100 pixels total, RBG color order",
        voltage: 12,
        chip: "ws2811",
        color_order: "rbg",
        leds_per_meter: 60,
        leds_per_ic: 3,
        pixels_per_meter: 20,
        total_length_m: 5.0,
        total_pixels: 100,
        total_physical_leds: 300,
        recommended_psu_ma: 6000,
        power_injection_notes: "12V strips drop noticeably over 5m (~72W / ~6.0A full load). Without injection, the far end may drop below 10V, causing pink/yellow color shift. Inject 12V and GND at both ends (0m and 5m).",
        wiring_notes: "Note the RBG color order (Blue and Green swapped in factory batch). Common GND connection between 12V PSU and controller is strictly required. Use 330 ohm series resistor on data line near first pixel. Keep data lead under 2-3 meters or use twisted pair / shielded cable.",
    },
    TapeProfile {
        id: "ws2812b_60led_5v_5m",
        aliases: &[
            "ws2812b",
            "ws2812b-5v",
            "ws2812b_5v_5m",
            "ws2812b-5v-5m",
            "generic-5v",
            "3",
        ],
        name: "Generic WS2812B 60 LEDs/m 5V (5m)",
        description: "Discrete addressable 5050 SMD tape, 1 LED/IC (60 px/m), 300 pixels total, GRB color order",
        voltage: 5,
        chip: "ws2812b",
        color_order: "grb",
        leds_per_meter: 60,
        leds_per_ic: 1,
        pixels_per_meter: 60,
        total_length_m: 5.0,
        total_pixels: 300,
        total_physical_leds: 300,
        recommended_psu_ma: 15000,
        power_injection_notes: "5V suffers from steep voltage drops. Inject 5V and GND at both ends and at least every 2 to 2.5 meters. Use heavy gauge feed wire (AWG 14-16) from distribution block.",
        wiring_notes: "GRB color order. Requires 3.3V to 5V logic level conversion (74AHCT125) on Raspberry Pi. Connect PSU and Pi grounds together. 330-470 ohm resistor on data line.",
    },
];

impl TapeProfile {
    /// Return the entire catalog of built-in profiles.
    pub fn catalog() -> &'static [TapeProfile] {
        BUILTIN_PROFILES
    }

    /// Search the catalog by id or alias (case-insensitive).
    pub fn find(query: &str) -> Option<&'static TapeProfile> {
        let q = query.trim().to_lowercase();
        for p in BUILTIN_PROFILES {
            if p.id.to_lowercase() == q {
                return Some(p);
            }
            for &alias in p.aliases {
                if alias.to_lowercase() == q {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Generate a fully-annotated TOML configuration file string based on this profile.
    pub fn generate_toml(
        &self,
        driver: &str,
        device: &str,
        baud_rate: u32,
        personality: &str,
        universe: u16,
        address: u16,
    ) -> String {
        let last_px_idx = if self.total_pixels > 0 {
            self.total_pixels - 1
        } else {
            0
        };

        format!(
            r#"# PixelNode Configuration
# Generated from profile: {name} ({id})
# Total Pixels: {pixels} | Voltage: {voltage}V | Color Order: {color_order}

[node]
name = "PixelNode-{id_short}"
show_lock = false               # If true, locks configuration changes during live show

[network]
artnet = true                   # Listen on UDP 6454
sacn = true                     # Listen on UDP 5568 & join E1.31 multicast groups
bind_ip = "0.0.0.0"

[web]
enabled = true                  # Embedded Web UI & REST API
port = 8080                     # Web UI port (http://<ip>:8080)
bind_ip = "0.0.0.0"

[audio]
enabled = true
listen_udp_port = 8765          # Broadcast port from audio-analyzerd or host sender
timeout_ms = 500               # Revert to neutral modulation when host audio stops

[failsafe]
on_data_loss = "hold"           # hold (freeze last frame) | hold_then_fade | blackout
hold_timeout_sec = 10          # Seconds to hold before fadeout (if hold_then_fade)
fade_duration_sec = 3           # Fadeout duration in seconds

[[output]]
id = "out1"
driver = "{driver}"             # spi_ws281x (Raspberry Pi SPI) | serial_mcu (Arduino Uno USB) | virtual
device = "{device}"             # /dev/spidev0.0 or /dev/ttyUSB0
baud_rate = {baud_rate}          # Serial baud rate (115200 for Arduino Uno)
chip = "{chip}"                 # ws2811 | ws2812b
color_order = "{color_order}"             # Strip color order ({color_order})
pixels = {pixels}                    # Total controlled pixels ({pixels} ICs = {physical_leds} physical LEDs)
psu_max_current_ma = {psu_ma}       # Automatic Brightness Limiter (ABL) current cap

[[fixture]]
output = "out1"
pixel_range = [0, {last_px_idx}]          # Pixel index range [start, end]
personality = "{personality}"      # preset_7ch (7ch address saver) | standard (12ch) | pixel_direct (RGB)
white_mode = "rgb_blend"        # rgb_blend (expand White to RGB) | ignore
universe_span = "per_170px"     # per_170px (170px per universe) | packed
universe = {universe}                    # DMX Start Universe (Art-Net / sACN)
address = {address}                     # DMX Start Channel (1..=512)

# ==============================================================================
# FIELD INSTALLATION & WIRING NOTES
# ==============================================================================
# Power Injection:
#   {power_notes}
#
# Signal & Grounding:
#   {wiring_notes}
# ==============================================================================
"#,
            name = self.name,
            id = self.id,
            id_short = self.id.split('_').next().unwrap_or("node"),
            pixels = self.total_pixels,
            voltage = self.voltage,
            color_order = self.color_order,
            driver = driver,
            device = device,
            baud_rate = baud_rate,
            chip = self.chip,
            physical_leds = self.total_physical_leds,
            psu_ma = self.recommended_psu_ma,
            last_px_idx = last_px_idx,
            personality = personality,
            universe = universe,
            address = address,
            power_notes = self.power_injection_notes,
            wiring_notes = self.wiring_notes,
        )
    }
}

/// Dynamically calculated profile for custom LED strip setups.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomTapeConfig {
    pub name: String,
    pub voltage: u32,
    pub chip: String,
    pub color_order: String,
    pub leds_per_meter: u32,
    pub leds_per_ic: u32,
    pub length_meters: f32,
    pub total_pixels: usize,
    pub total_physical_leds: usize,
    pub psu_max_current_ma: u32,
}

impl CustomTapeConfig {
    /// Calculate pixel count and recommended current cap for a custom strip.
    pub fn new(
        name: impl Into<String>,
        voltage: u32,
        chip: impl Into<String>,
        color_order: impl Into<String>,
        leds_per_meter: u32,
        leds_per_ic: u32,
        length_meters: f32,
    ) -> Self {
        let safe_leds_per_ic = leds_per_ic.max(1);
        let px_per_m = leds_per_meter / safe_leds_per_ic;
        let total_pixels = ((px_per_m as f32) * length_meters).round() as usize;
        let total_physical_leds = ((leds_per_meter as f32) * length_meters).round() as usize;

        // Estimated power draw based on voltage and length
        let psu_ma = match voltage {
            24 => {
                // ~15W per meter at 24V -> ~625 mA/m
                ((length_meters * 625.0) * 1.15).round() as u32
            }
            12 => {
                // ~14.4W per meter at 12V -> ~1200 mA/m
                ((length_meters * 1200.0) * 1.15).round() as u32
            }
            _ => {
                // 5V default -> ~50mA per pixel
                (total_pixels as u32 * 50).max(1000)
            }
        };

        Self {
            name: name.into(),
            voltage,
            chip: chip.into(),
            color_order: color_order.into(),
            leds_per_meter,
            leds_per_ic: safe_leds_per_ic,
            length_meters,
            total_pixels: total_pixels.max(1),
            total_physical_leds,
            psu_max_current_ma: psu_ma,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_profiles_presence() {
        let catalog = TapeProfile::catalog();
        assert!(catalog.len() >= 2);

        let btf = TapeProfile::find("btf_fcob_576_24v_10m").expect("BTF profile missing");
        assert_eq!(btf.voltage, 24);
        assert_eq!(btf.total_pixels, 160);
        assert_eq!(btf.leds_per_meter, 576);
        assert_eq!(btf.leds_per_ic, 36);
        assert_eq!(btf.color_order, "rgb");

        let nobrand = TapeProfile::find("nobrand_60led_12v_5m").expect("No-brand profile missing");
        assert_eq!(nobrand.voltage, 12);
        assert_eq!(nobrand.total_pixels, 100);
        assert_eq!(nobrand.leds_per_meter, 60);
        assert_eq!(nobrand.leds_per_ic, 3);
        assert_eq!(nobrand.color_order, "rbg");
    }

    #[test]
    fn test_profile_aliases_and_lookup() {
        assert_eq!(
            TapeProfile::find("1"),
            TapeProfile::find("btf_fcob_576_24v_10m")
        );
        assert_eq!(
            TapeProfile::find("btf-fcob"),
            TapeProfile::find("btf_fcob_576_24v_10m")
        );
        assert_eq!(
            TapeProfile::find("generic-12v-5m"),
            TapeProfile::find("nobrand_60led_12v_5m")
        );
        assert_eq!(
            TapeProfile::find("2"),
            TapeProfile::find("nobrand_60led_12v_5m")
        );
        assert!(TapeProfile::find("nonexistent_tape_id").is_none());
    }

    #[test]
    fn test_generate_toml_content() {
        let btf = TapeProfile::find("btf_fcob_576_24v_10m").unwrap();
        let toml_str = btf.generate_toml(
            "spi_ws281x",
            "/dev/spidev0.0",
            115200,
            "preset_7ch",
            1,
            1,
        );

        assert!(toml_str.contains("pixels = 160"));
        assert!(toml_str.contains("color_order = \"rgb\""));
        assert!(toml_str.contains("chip = \"ws2811\""));
        assert!(toml_str.contains("pixel_range = [0, 159]"));
        assert!(toml_str.contains("driver = \"spi_ws281x\""));
    }

    #[test]
    fn test_custom_tape_calculation() {
        let custom = CustomTapeConfig::new(
            "Custom 24V Strip",
            24,
            "ws2811",
            "rgb",
            576,
            36,
            10.0,
        );
        assert_eq!(custom.total_pixels, 160);
        assert_eq!(custom.total_physical_leds, 5760);
        assert!(custom.psu_max_current_ma >= 6000);
    }
}
