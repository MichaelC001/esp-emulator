//! A TowerPro SG90 micro servo, written as a virtual part.
//!
//! A part is a WebAssembly module that models a component wired to the chip's
//! pins. This one is a hobby servo on one signal pin: the firmware sends it a
//! pulse every 20 ms, and the width of the pulse is the angle the horn goes
//! to. The SG90 datasheet gives the mapping: 1.5 ms is the middle, about 2 ms
//! all the way one way and about 1 ms all the way the other, over about 180
//! degrees. Here 1 ms is 0 degrees and 2 ms is 180. README.md lists what it
//! leaves out.
//!
//! The servo does not count edges: it claims its pin as PWM (`reads_pwm`), so
//! the emulator tells it the period and the high time whenever they change,
//! from LEDC or from an MCPWM generator alike.
//!
//! `Sg90` is the device and knows nothing about WebAssembly. The
//! `export_part!` line at the end makes it a part: the `esp-part` crate exports
//! every function of `proto/PART_ABI.md`, `part_on_message` included, and moves
//! the bytes between the shared buffer and the device.

use esp_part::prelude::*;

/// The servo's own name for its signal lead. The board says which chip pin it
/// is wired to, as `{ pins: { SIG: 4 } }`.
const SIG: &str = "SIG";

/// The pulse widths at the two ends of the travel (the datasheet's -90 and
/// +90 positions). A pulse outside them holds the horn at the nearer end.
const MIN_PULSE_NS: u64 = 1_000_000;
const MAX_PULSE_NS: u64 = 2_000_000;
/// The travel, in degrees.
const SWEEP: f32 = 180.0;

/// The periods a hobby servo holds position on: 330 Hz to 40 Hz. The SG90 is
/// specified at 20 ms; outside the band the horn does not hold, which is
/// reported as not driven rather than smoothed over.
const MIN_PERIOD_NS: u64 = 3_030_303;
const MAX_PERIOD_NS: u64 = 25_000_000;

#[derive(Default)]
struct Sg90 {
    /// The chip pin the signal lead is wired to; `None` when the board left
    /// it unconnected.
    pin: Option<u32>,
    /// The waveform on the pin, as last told.
    period_ns: u64,
    pulse_ns: u64,
    /// Where the horn is, in degrees from one end. `None` before the first
    /// pulse: a servo never driven is wherever it was left.
    angle: Option<f32>,
}

impl Sg90 {
    /// The angle a pulse of `pulse_ns` commands.
    fn angle_for(pulse_ns: u64) -> f32 {
        let pulse = pulse_ns.clamp(MIN_PULSE_NS, MAX_PULSE_NS) - MIN_PULSE_NS;
        pulse as f32 / (MAX_PULSE_NS - MIN_PULSE_NS) as f32 * SWEEP
    }

    /// Whether the pin carries pulses at a rate the servo holds position on.
    /// A pin held low, held high or not driven at all is not: the motor stops
    /// driving and the horn stays where it is.
    fn is_driven(&self) -> bool {
        (MIN_PERIOD_NS..=MAX_PERIOD_NS).contains(&self.period_ns)
            && self.pulse_ns > 0
            && self.pulse_ns < self.period_ns
    }
}

impl Device for Sg90 {
    const NAME: &'static str = "sg90";
    const ON_MESSAGE: bool = true;

    /// Where the board wired the part.
    fn configure(&mut self, config: &PartConfig) {
        self.pin = config.pins.get(SIG).copied().filter(|&p| p < 64);
    }

    /// Its signal pin, read as PWM. Nothing wired, nothing claimed.
    fn claims(&self) -> Vec<Claim> {
        self.pin.map(|pin| reads_pwm(1 << pin)).into_iter().collect()
    }

    /// The waveform on the pin changed. The horn follows a pulse it holds
    /// position on, and stays put otherwise.
    fn on_pwm(&mut self, state: &PwmState) {
        if Some(state.pin) != self.pin {
            return;
        }
        self.period_ns = state.period_ns;
        self.pulse_ns = state.high_ns;
        if self.is_driven() {
            self.angle = Some(Self::angle_for(self.pulse_ns));
        }
    }

    fn state(&self) -> Option<State> {
        let state = State::new().set("driven", self.is_driven()).set("pulse_us", self.pulse_ns / 1000);
        Some(match self.angle {
            Some(angle) => state.set("angle", format!("{angle:.1}")),
            None => state,
        })
    }
}

esp_part::export_part!(Sg90, buffer = 256);

#[cfg(test)]
mod tests {
    use super::*;

    /// A servo wired to GPIO 4.
    fn servo() -> Sg90 {
        let mut s = Sg90::default();
        s.configure(&PartConfig { pins: [(SIG.to_string(), 4)].into(), ..Default::default() });
        s
    }

    fn pulse(s: &mut Sg90, period_ns: u64, high_ns: u64) {
        s.on_pwm(&PwmState { pin: 4, period_ns, high_ns });
    }

    #[test]
    fn the_pulse_width_is_the_angle() {
        let mut s = servo();
        for (us, deg) in [(1000, 0.0), (1500, 90.0), (2000, 180.0), (1250, 45.0)] {
            pulse(&mut s, 20_000_000, us * 1000);
            assert_eq!(s.angle, Some(deg), "{us} us");
        }
    }

    #[test]
    fn a_pulse_past_the_ends_holds_the_nearer_end() {
        let mut s = servo();
        pulse(&mut s, 20_000_000, 500_000);
        assert_eq!(s.angle, Some(0.0));
        pulse(&mut s, 20_000_000, 2_600_000);
        assert_eq!(s.angle, Some(180.0));
    }

    #[test]
    fn a_pin_not_driven_leaves_the_horn_where_it_is() {
        let mut s = servo();
        assert_eq!(s.angle, None, "never driven: nowhere in particular");
        pulse(&mut s, 20_000_000, 1_500_000);
        for (period, high) in [(0, 0), (20_000_000, 0), (20_000_000, 20_000_000), (1_000_000, 500_000)] {
            pulse(&mut s, period, high);
            assert!(!s.is_driven(), "{period}/{high}");
            assert_eq!(s.angle, Some(90.0), "{period}/{high}: held");
        }
    }

    #[test]
    fn another_pin_is_not_its_own() {
        let mut s = servo();
        s.on_pwm(&PwmState { pin: 5, period_ns: 20_000_000, high_ns: 2_000_000 });
        assert_eq!(s.angle, None);
    }

    #[test]
    fn it_claims_its_pin_as_pwm_and_nothing_unwired() {
        assert_eq!(servo().claims(), [reads_pwm(1 << 4)]);
        assert!(Sg90::default().claims().is_empty());
    }

    #[test]
    fn it_shows_the_angle_and_whether_it_is_driven() {
        let mut s = servo();
        pulse(&mut s, 20_000_000, 1_500_244);
        let state = s.state().unwrap();
        assert_eq!(state.get("angle"), Some("90.0"));
        assert_eq!(state.get("driven"), Some("true"));
        assert_eq!(state.get("pulse_us"), Some("1500"));
    }
}
