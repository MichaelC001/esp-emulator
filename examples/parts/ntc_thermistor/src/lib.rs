//! An NTC thermistor in a voltage divider, written as a virtual part.
//!
//! A part is a WebAssembly module that models a component wired to the chip's
//! pins. This one is the common thermistor module: a 10 kΩ fixed resistor from
//! the 3.3 V rail to the output and a 10 kΩ (at 25 °C) NTC thermistor, B 3950,
//! from the output to ground. The output is on an ADC pad, so the firmware
//! reads a voltage that falls as the thermistor warms:
//!
//! ```text
//!   R(T) = R25 · exp(B · (1/T − 1/T25))        T in kelvin
//!   V    = 3300 mV · R(T) / (R_FIXED + R(T))   1650 mV at 25 °C
//! ```
//!
//! `Thermistor` is the device and knows nothing about WebAssembly. The
//! `export_part!` line at the end makes it a part.

use esp_part::prelude::*;

/// The module's own name for its output pin. The board says which chip pin
/// it is wired to, as `{ pins: { OUT: 3 } }`.
const OUT: &str = "OUT";

/// The rail across the divider, in millivolts.
const RAIL_MV: f64 = 3300.0;
/// The fixed resistor, from the rail to the output, in ohms.
const R_FIXED: f64 = 10_000.0;
/// The thermistor's resistance at 25 °C, in ohms, and its B constant (25/85).
const R25: f64 = 10_000.0;
const B: f64 = 3950.0;
/// 25 °C in kelvin.
const T25_K: f64 = 298.15;
/// The range the thermistor is specified over, in °C. A temperature outside
/// it reads as its nearest end.
const MIN_C: f64 = -40.0;
const MAX_C: f64 = 125.0;

#[derive(Default)]
struct Thermistor {
    /// The chip pin the output is wired to. `None` until the board says, or
    /// when it left the pin unconnected.
    pin: Option<u32>,
    /// The thermistor's temperature, in °C.
    temp_c: f64,
    /// What the emulator was last told, in millivolts.
    reported: Option<u32>,
}

impl Thermistor {
    /// The thermistor's resistance at its temperature, in ohms.
    fn ohms(&self) -> f64 {
        R25 * (B * (1.0 / (self.temp_c + 273.15) - 1.0 / T25_K)).exp()
    }

    /// The divider's output, in millivolts.
    fn millivolts(&self) -> u32 {
        let r = self.ohms();
        (RAIL_MV * r / (R_FIXED + r)).round() as u32
    }
}

impl Device for Thermistor {
    const NAME: &'static str = "ntc_thermistor";

    /// Where the board wired the part. It starts at 25 °C.
    fn configure(&mut self, config: &PartConfig) {
        self.pin = config.pins.get(OUT).copied();
        self.temp_c = 25.0;
    }

    /// The pad the output is on.
    fn claims(&self) -> Vec<Claim> {
        self.pin.map(analog).into_iter().collect()
    }

    /// The output's voltage when it moved.
    fn poll(&mut self, now_ns: u64) -> Option<EmulatorMessage> {
        let pin = self.pin?;
        let mv = self.millivolts();
        if self.reported == Some(mv) {
            return None;
        }
        self.reported = Some(mv);
        Some(hold(pin, mv, now_ns))
    }

    /// Somebody warmed or cooled the thermistor. `set:temperature=<°C>` puts
    /// it at a temperature in one step.
    fn on_input(&mut self, name: &str) {
        let t = match set_value(name) {
            Some(("temperature", value)) => value,
            Some(_) => return, // Not a value this part has.
            None => match name {
                "warmer" => self.temp_c + 1.0,
                "cooler" => self.temp_c - 1.0,
                _ => return, // Not an interaction this part has.
            },
        };
        self.temp_c = t.clamp(MIN_C, MAX_C);
    }

    fn state(&self) -> Option<State> {
        Some(State::new().set("temperature", self.temp_c).set("millivolts", self.millivolts()))
    }
}

esp_part::export_part!(Thermistor);

#[cfg(test)]
mod tests {
    use super::*;

    /// Wired as README.md's board wires it: the output on GPIO 3.
    fn sensor() -> Thermistor {
        let mut t = Thermistor::default();
        t.configure(&PartConfig { pins: [(OUT.to_string(), 3)].into(), ..Default::default() });
        t
    }

    fn held(t: &mut Thermistor) -> Option<(u32, u32)> {
        match t.poll(0)?.payload? {
            esp_part::Payload::AnalogDrive(a) => Some((a.pin, a.millivolts)),
            other => panic!("not an AnalogDrive: {other:?}"),
        }
    }

    #[test]
    fn at_25_c_the_divider_is_half_the_rail() {
        let mut t = sensor();
        assert_eq!(t.claims(), vec![analog(3)]);
        assert_eq!(held(&mut t), Some((3, 1650)));
        assert_eq!(held(&mut t), None, "said once, then held");
    }

    #[test]
    fn warming_it_lowers_the_output() {
        // B 3950: R(50 °C) = 3588 Ω, R(0 °C) = 33 621 Ω.
        let mut t = sensor();
        t.on_input("set:temperature=50");
        assert_eq!(held(&mut t), Some((3, 871)));
        t.on_input("set:temperature=0");
        assert_eq!(held(&mut t), Some((3, 2543)));
        t.on_input("warmer");
        assert!(held(&mut t).unwrap().1 < 2543);
        t.on_input("set:temperature=500");
        assert_eq!(t.temp_c, MAX_C, "held to the specified range");
        t.on_input("wiggle");
        assert_eq!(t.temp_c, MAX_C);
    }

    #[test]
    fn an_unwired_module_claims_and_holds_nothing() {
        let mut t = Thermistor::default();
        t.configure(&PartConfig::default());
        assert!(t.claims().is_empty());
        assert_eq!(held(&mut t), None);
    }
}
