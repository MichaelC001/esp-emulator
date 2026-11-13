//! A potentiometer, written as a virtual part.
//!
//! A part is a WebAssembly module that models a component wired to the chip's
//! pins. This one is a linear potentiometer between the 3.3 V rail and ground
//! with its wiper on an ADC pad: the wiper's voltage is the knob's position
//! times the rail. The firmware reads it with the stock `adc_oneshot` driver.
//!
//! `Potentiometer` is the device and knows nothing about WebAssembly. The
//! `export_part!` line at the end makes it a part: the `esp-part` crate
//! exports every function of `proto/PART_ABI.md` and moves the bytes between
//! the shared buffer and the device.

use esp_part::prelude::*;

/// The potentiometer's own name for its wiper pin. The board says which chip
/// pin it is wired to, as `{ pins: { W: 3 } }`.
const WIPER: &str = "W";

/// The rail across the track, in millivolts.
const RAIL_MV: u32 = 3300;
/// What `more` and `less` turn the knob by, in percent.
const STEP: u32 = 10;

#[derive(Default)]
struct Potentiometer {
    /// The chip pin the wiper is wired to. `None` until the board says, or
    /// when it left the pin unconnected: an unwired knob holds nothing.
    pin: Option<u32>,
    /// The knob's position along the track, 0 to 100 percent.
    position: u32,
    /// What the emulator was last told. `poll` speaks only when the wiper's
    /// voltage differs from it, so a turn is one message, not one per poll.
    reported: Option<u32>,
}

impl Potentiometer {
    /// The wiper's voltage, in millivolts.
    fn millivolts(&self) -> u32 {
        RAIL_MV * self.position / 100
    }
}

impl Device for Potentiometer {
    const NAME: &'static str = "potentiometer";

    /// Where the board wired the part. The knob starts half way.
    fn configure(&mut self, config: &PartConfig) {
        self.pin = config.pins.get(WIPER).copied();
        self.position = 50;
    }

    /// The pad the wiper is on. The emulator refuses a pad with no ADC
    /// channel behind it.
    fn claims(&self) -> Vec<Claim> {
        self.pin.map(analog).into_iter().collect()
    }

    /// The wiper's voltage when it moved: held on the pad until the next one,
    /// across a chip reset.
    fn poll(&mut self, now_ns: u64) -> Option<EmulatorMessage> {
        let pin = self.pin?;
        let mv = self.millivolts();
        if self.reported == Some(mv) {
            return None;
        }
        self.reported = Some(mv);
        Some(hold(pin, mv, now_ns))
    }

    /// Somebody turned the knob. `set:position=<percent>` puts it anywhere
    /// in one step.
    fn on_input(&mut self, name: &str) {
        match set_value(name) {
            Some(("position", value)) => self.position = value.round().clamp(0.0, 100.0) as u32,
            Some(_) => {} // Not a value this part has.
            None => match name {
                "more" => self.position = (self.position + STEP).min(100),
                "less" => self.position = self.position.saturating_sub(STEP),
                _ => {} // Not an interaction this part has.
            },
        }
    }

    fn state(&self) -> Option<State> {
        Some(State::new().set("position", self.position).set("millivolts", self.millivolts()))
    }
}

esp_part::export_part!(Potentiometer);

// The device runs on the host too, so `cargo test` checks it without an
// emulator.
#[cfg(test)]
mod tests {
    use super::*;

    /// Wired as README.md's board wires it: the wiper on GPIO 3.
    fn knob() -> Potentiometer {
        let mut k = Potentiometer::default();
        k.configure(&PartConfig { pins: [(WIPER.to_string(), 3)].into(), ..Default::default() });
        k
    }

    /// The pad and millivolts of what `poll` sends, if anything.
    fn held(k: &mut Potentiometer) -> Option<(u32, u32)> {
        match k.poll(0)?.payload? {
            esp_part::Payload::AnalogDrive(a) => Some((a.pin, a.millivolts)),
            other => panic!("not an AnalogDrive: {other:?}"),
        }
    }

    #[test]
    fn it_claims_the_wiper_pad_and_starts_half_way() {
        let mut k = knob();
        assert_eq!(k.claims(), vec![analog(3)]);
        assert_eq!(held(&mut k), Some((3, 1650)));
        assert_eq!(held(&mut k), None, "said once, then held");
    }

    #[test]
    fn turning_it_moves_the_wiper() {
        let mut k = knob();
        held(&mut k);
        k.on_input("more");
        assert_eq!(held(&mut k), Some((3, 1980)));
        k.on_input("set:position=20");
        assert_eq!(held(&mut k), Some((3, 660)));
        for _ in 0..5 {
            k.on_input("less");
        }
        assert_eq!(held(&mut k), Some((3, 0)), "stops at the end of the track");
        k.on_input("set:position=250");
        assert_eq!(held(&mut k), Some((3, 3300)));
        k.on_input("wiggle");
        assert_eq!(held(&mut k), None, "an interaction it lacks changes nothing");
    }

    #[test]
    fn an_unwired_knob_claims_and_holds_nothing() {
        let mut k = Potentiometer::default();
        k.configure(&PartConfig::default());
        assert!(k.claims().is_empty());
        assert_eq!(held(&mut k), None);
    }

    #[test]
    fn it_shows_its_position_and_voltage() {
        let k = knob();
        let s = k.state().unwrap();
        assert_eq!((s.get("position"), s.get("millivolts")), (Some("50"), Some("1650")));
    }
}
