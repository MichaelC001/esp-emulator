//! A Winsen MH-Z19B infrared CO₂ sensor, written as a virtual part.
//!
//! A part is a WebAssembly module that models a component wired to the chip's
//! pins. This one is on a serial line, 9600 baud, 8N1. The firmware sends a
//! nine-byte command, `FF 01 86 00 00 00 00 00 79` to read the concentration,
//! and the sensor answers `FF 86 HH LL 00 00 00 00 CS`: the concentration in
//! ppm is `HH * 256 + LL`, and `CS` is the two's complement of the sum of
//! bytes 1 to 7, as in every frame. It follows the "MH-Z19B Intelligent
//! Infrared Gas Module User's Manual" (Winsen, V1.0); README.md lists what it
//! leaves out.
//!
//! `Mhz19b` is the device and knows nothing about WebAssembly. The
//! `export_part!` line at the end makes it a part: the `esp-part` crate exports
//! every function of `proto/PART_ABI.md`, `part_on_message` included, which
//! hands the device what the chip transmits, and moves the bytes between the
//! shared buffer and the device.

use esp_part::prelude::*;

/// The sensor's own names for its pins (the manual's pin table). The board
/// says which chip pins they are wired to, as `{ pins: { TX: 5, RX: 4 } }`:
/// the sensor's TX goes to the chip's RX.
const TX: &str = "TX";
const RX: &str = "RX";

/// The first byte of every frame, either way.
const START: u8 = 0xFF;
/// The command that reads the concentration (the manual's command table).
const READ_CO2: u8 = 0x86;
/// A frame's length, either way.
const FRAME: usize = 9;

/// The range the manual's specification table gives the sensor, 0 to 5000
/// ppm. A concentration outside it reads as its nearest end.
const MAX_PPM: u32 = 5000;
/// What `more` and `less` move the concentration by.
const STEP_PPM: u32 = 100;

struct Mhz19b {
    /// The chip pins the sensor's own pins are wired to. `None` until the
    /// board says, or when it left the pin unconnected.
    tx: Option<u32>,
    rx: Option<u32>,
    /// The CO₂ concentration around the sensor, in ppm.
    ppm: u32,
    /// The command received so far: the line delivers bytes in pieces, so a
    /// frame may arrive over several calls.
    command: Vec<u8>,
    /// Answers not yet sent.
    answers: Vec<u8>,
}

impl Default for Mhz19b {
    fn default() -> Self {
        // Fresh outdoor air, which is also what the sensor's zero point
        // calibration takes the air around it to be.
        Self { tx: None, rx: None, ppm: 400, command: Vec::new(), answers: Vec::new() }
    }
}

/// The checksum of a frame: the two's complement of the sum of bytes 1 to 7
/// (the manual's "Checksum calculation").
fn checksum(frame: &[u8]) -> u8 {
    frame[1..8].iter().fold(0u8, |sum, b| sum.wrapping_add(*b)).wrapping_neg()
}

impl Device for Mhz19b {
    const NAME: &'static str = "mhz19b";
    const ON_MESSAGE: bool = true;

    /// Where the board wired the part.
    fn configure(&mut self, config: &PartConfig) {
        self.tx = config.pins.get(TX).copied();
        self.rx = config.pins.get(RX).copied();
    }

    /// The serial line the sensor is on, by its own TX and RX pins. The
    /// emulator works out which UART that is from the firmware's pin setup.
    /// A sensor the board left without one of them still claims, with it
    /// left out, so the emulator refuses the claim and says so.
    fn claims(&self) -> Vec<Claim> {
        vec![serial(self.tx, self.rx)]
    }

    /// What the chip sent. A frame starts with 0xFF; anything before one is
    /// noise the sensor skips, and a frame with a bad checksum is dropped
    /// from its first byte on, so the next 0xFF in it can start one.
    fn on_uart(&mut self, data: &[u8]) {
        for &b in data {
            if self.command.is_empty() && b != START {
                continue;
            }
            self.command.push(b);
            if self.command.len() < FRAME {
                continue;
            }
            if checksum(&self.command) == self.command[8] {
                if self.command[2] == READ_CO2 {
                    let answer = self.reading();
                    self.answers.extend_from_slice(&answer);
                }
                // Calibration and range commands have no answer, and their
                // effects are not modelled.
                self.command.clear();
            } else {
                let rest = self.command.split_off(1);
                self.command.clear();
                if let Some(at) = rest.iter().position(|&b| b == START) {
                    self.command.extend_from_slice(&rest[at..]);
                }
            }
        }
    }

    /// An answer, as soon as there is one: at 9600 baud the real sensor's
    /// reply follows the command within a few frame times.
    fn poll(&mut self, now_ns: u64) -> Option<EmulatorMessage> {
        if self.answers.is_empty() {
            return None;
        }
        Some(transmit(&std::mem::take(&mut self.answers), now_ns))
    }

    /// Somebody changed the air around the sensor. The names are the part's
    /// own, and `set:co2=<ppm>` puts a value in one step.
    fn on_input(&mut self, name: &str) {
        match set_value(name) {
            Some(("co2", value)) => self.ppm = value.round().clamp(0.0, MAX_PPM as f64) as u32,
            Some(_) => {} // Not a value this part has.
            None => match name {
                "more" => self.ppm = (self.ppm + STEP_PPM).min(MAX_PPM),
                "less" => self.ppm = self.ppm.saturating_sub(STEP_PPM),
                _ => {} // Not an interaction this part has.
            },
        }
    }

    fn state(&self) -> Option<State> {
        Some(State::new().set("co2", self.ppm))
    }
}

impl Mhz19b {
    /// The answer to a read: the concentration, high byte first; bytes 4 to
    /// 7 are not defined by the manual and read 0.
    fn reading(&self) -> [u8; FRAME] {
        let [hi, lo] = (self.ppm as u16).to_be_bytes();
        let mut frame = [START, READ_CO2, hi, lo, 0, 0, 0, 0, 0];
        frame[8] = checksum(&frame);
        frame
    }
}

// A host hands over what the chip transmits 128 bytes at a time, in a message
// of up to 147 bytes; the answer is a nine-byte `UartRx`.
esp_part::export_part!(Mhz19b, buffer = 256);

// The device runs on the host too, so `cargo test` checks it without an
// emulator.
#[cfg(test)]
mod tests {
    use super::*;

    /// The manual's read command.
    const READ: [u8; 9] = [0xFF, 0x01, 0x86, 0x00, 0x00, 0x00, 0x00, 0x00, 0x79];

    /// Wired as README.md's board wires it: TX on GPIO 5, RX on GPIO 4.
    fn sensor() -> Mhz19b {
        let mut sensor = Mhz19b::default();
        let pins = [(TX.to_string(), 5), (RX.to_string(), 4)].into();
        sensor.configure(&PartConfig { pins, ..Default::default() });
        sensor
    }

    /// The bytes the sensor sends back, if any.
    fn answer(sensor: &mut Mhz19b) -> Vec<u8> {
        match sensor.poll(7).and_then(|m| m.payload) {
            Some(esp_part::Payload::UartRx(rx)) => rx.data,
            None => Vec::new(),
            Some(other) => panic!("not a UartRx: {other:?}"),
        }
    }

    #[test]
    fn the_read_commands_checksum_is_the_manuals() {
        assert_eq!(checksum(&READ), 0x79);
    }

    #[test]
    fn a_read_is_answered_with_the_concentration() {
        let mut sensor = sensor();
        sensor.on_uart(&READ);
        // 400 ppm is 0x0190; 0x86 + 0x01 + 0x90 = 0x117, so CS = 0xE9.
        assert_eq!(answer(&mut sensor), [0xFF, 0x86, 0x01, 0x90, 0, 0, 0, 0, 0xE9]);
        assert_eq!(answer(&mut sensor), [], "answered once");
    }

    #[test]
    fn a_command_in_pieces_is_answered_once_whole() {
        let mut sensor = sensor();
        sensor.on_uart(&READ[..4]);
        assert_eq!(answer(&mut sensor), []);
        sensor.on_uart(&READ[4..]);
        assert_eq!(answer(&mut sensor).len(), 9);
        // Two commands in one piece: two answers.
        sensor.on_uart(&[READ, READ].concat());
        assert_eq!(answer(&mut sensor).len(), 18);
    }

    #[test]
    fn noise_and_bad_frames_are_skipped() {
        let mut sensor = sensor();
        let mut bad = READ;
        bad[8] = 0x78;
        sensor.on_uart(&[0x00, 0x42]);
        sensor.on_uart(&bad);
        assert_eq!(answer(&mut sensor), [], "a bad checksum is not answered");
        sensor.on_uart(&READ);
        assert_eq!(answer(&mut sensor).len(), 9, "the next good frame is");
        // A frame cut short by a new start: the 0xFF inside starts again.
        sensor.on_uart(&[0xFF, 0x01, 0x86]);
        sensor.on_uart(&READ);
        assert_eq!(answer(&mut sensor).len(), 9);
    }

    #[test]
    fn other_commands_have_no_answer() {
        // Zero point calibration (0x87): CS = -(0x01 + 0x87) = 0x78.
        let mut sensor = sensor();
        sensor.on_uart(&[0xFF, 0x01, 0x87, 0, 0, 0, 0, 0, 0x78]);
        assert_eq!(answer(&mut sensor), []);
    }

    #[test]
    fn its_claim_names_the_line_from_the_chips_end() {
        let line = UartClaim { tx_pin: Some(4), rx_pin: Some(5) };
        assert_eq!(sensor().claims(), [Claim { what: Some(claim::What::Uart(line)) }]);
        // Unwired: claimed without its pads, which the handshake refuses.
        let none = UartClaim::default();
        assert_eq!(Mhz19b::default().claims(), [Claim { what: Some(claim::What::Uart(none)) }]);
    }

    #[test]
    fn the_inputs_move_the_reading_and_stop_at_the_range() {
        let mut sensor = sensor();
        sensor.on_input("more");
        sensor.on_input("wiggle");
        assert_eq!(sensor.ppm, 500);
        for _ in 0..100 {
            sensor.on_input("more");
        }
        assert_eq!(sensor.ppm, 5000);
        for _ in 0..100 {
            sensor.on_input("less");
        }
        assert_eq!(sensor.ppm, 0);
    }

    #[test]
    fn a_set_puts_an_exact_value_and_stays_in_range() {
        let mut sensor = sensor();
        sensor.on_input("set:co2=1200");
        sensor.on_uart(&READ);
        // 1200 is 0x04B0; 0x86 + 0x04 + 0xB0 = 0x13A, so CS = 0xC6.
        assert_eq!(answer(&mut sensor), [0xFF, 0x86, 0x04, 0xB0, 0, 0, 0, 0, 0xC6]);
        sensor.on_input("set:co2=9000");
        assert_eq!(sensor.ppm, 5000, "clamped to the range");
        for odd in ["set:co2=1e3", "set:co2=", "set:CO2=5", "set:ppm=5", "set:co2"] {
            sensor.on_input(odd);
        }
        assert_eq!(sensor.ppm, 5000, "anything else is ignored");
        assert_eq!(sensor.state().unwrap().get("co2"), Some("5000"));
    }
}
