//! A KY-040 rotary encoder module, written as a virtual part.
//!
//! The module carries an EC11-type mechanical encoder: 20 detents a turn, two
//! contacts that make a quadrature pair, CLK (the encoder's A) and DT (its
//! B), and a push switch, all closing to ground, with pull-ups on CLK and DT.
//! One detent walks the lines through `11 -> 01 -> 00 -> 10 -> 11` (CLK, DT),
//! one line at a time, and the line that moves first is the direction: CLK
//! first when turned clockwise, as IDF's `rotary_encoder` example draws the
//! EC11 waveform. The part counts nothing. The chip's PCNT does, from the
//! edges.
//!
//! One edge goes out per `poll`, stamped with the poll's time and at least
//! [`EDGE_GAP_NS`] after the one before. So the edges of a detent never land
//! at one instant, and none waits in the emulator's queue, which a chip reset
//! empties.

use esp_part::prelude::*;
use std::collections::VecDeque;

/// The module's pin names, as printed on it. The board says which chip pins
/// they are wired to, as `{ pins: { CLK: 5, DT: 6, SW: 7 } }`.
const CLK: &str = "CLK";
const DT: &str = "DT";
const SW: &str = "SW";

/// Shortest emulated time between two edges: a detent in 1 ms, faster than a
/// hand turns a knob. The emulator polls a part once a heartbeat (1 ms), so
/// this binds only when several interactions arrive at one time.
const EDGE_GAP_NS: u64 = 250_000;

#[derive(Default)]
struct Ky040 {
    /// The chip pins each line is wired to; `None` where the board left it
    /// unconnected.
    clk: Option<u32>,
    dt: Option<u32>,
    sw: Option<u32>,
    /// Edges still to send, in order: the pin and its new level.
    pending: VecDeque<(u32, bool)>,
    /// Where CLK and DT will be once `pending` has played out, low = `true`.
    /// A detent always ends with both high, where the next one starts.
    clk_low: bool,
    dt_low: bool,
    /// When the last edge went out.
    last_edge_ns: Option<u64>,
    /// The latest poll time: a clock that goes back is a chip reset.
    last_now_ns: u64,
    /// Detents turned, clockwise positive.
    position: i64,
    /// The switch, and what the emulator was last told of it.
    down: bool,
    reported_down: bool,
}

impl Ky040 {
    /// One detent: four transitions, the leading line toggled first each
    /// time. Queued whole, so a reversal starts from both lines high.
    fn turn(&mut self, cw: bool) {
        let (Some(clk), Some(dt)) = (self.clk, self.dt) else { return };
        for _ in 0..2 {
            for lead in [cw, !cw] {
                if lead {
                    self.clk_low = !self.clk_low;
                    self.pending.push_back((clk, !self.clk_low));
                } else {
                    self.dt_low = !self.dt_low;
                    self.pending.push_back((dt, !self.dt_low));
                }
            }
        }
        self.position += if cw { 1 } else { -1 };
    }
}

impl Device for Ky040 {
    const NAME: &'static str = "ky040";

    fn configure(&mut self, config: &PartConfig) {
        self.clk = config.pins.get(CLK).copied();
        self.dt = config.pins.get(DT).copied();
        self.sw = config.pins.get(SW).copied();
    }

    /// Every line the board wired, and only those.
    fn claims(&self) -> Vec<Claim> {
        let pins = [self.clk, self.dt, self.sw].into_iter().flatten().fold(0u64, |m, p| m | 1 << p);
        if pins == 0 { Vec::new() } else { vec![drives(pins)] }
    }

    /// `cw` and `ccw` turn the knob one detent; `press` and `release` work
    /// the switch.
    fn on_input(&mut self, name: &str) {
        match name {
            "cw" => self.turn(true),
            "ccw" => self.turn(false),
            "press" => self.down = true,
            "release" => self.down = false,
            _ => {}
        }
    }

    fn poll(&mut self, now_ns: u64) -> Option<EmulatorMessage> {
        if now_ns < self.last_now_ns {
            // The chip reset and its clock started again: the gap is
            // measured on the new clock.
            self.last_edge_ns = None;
        }
        self.last_now_ns = now_ns;
        // The switch shares no line with the rotation and goes first.
        if let Some(sw) = self.sw.filter(|_| self.down != self.reported_down) {
            self.reported_down = self.down;
            return Some(drive(sw, !self.down, now_ns));
        }
        if self.last_edge_ns.is_some_and(|last| now_ns < last + EDGE_GAP_NS) {
            return None;
        }
        let (pin, level) = self.pending.pop_front()?;
        self.last_edge_ns = Some(now_ns);
        Some(drive(pin, level, now_ns))
    }

    fn state(&self) -> Option<State> {
        Some(State::new().set("position", self.position).set("pressed", self.down))
    }
}

esp_part::export_part!(Ky040, buffer = 256);

#[cfg(test)]
mod tests {
    use super::*;
    use esp_part::Payload;

    const PIN_CLK: u32 = 5;
    const PIN_DT: u32 = 6;
    const PIN_SW: u32 = 7;

    fn wired() -> Ky040 {
        let mut k = Ky040::default();
        let pins = [(CLK, PIN_CLK), (DT, PIN_DT), (SW, PIN_SW)].map(|(n, p)| (n.to_string(), p));
        k.configure(&PartConfig { pins: pins.into(), ..Default::default() });
        k
    }

    fn drive_of(msg: EmulatorMessage) -> (u32, bool, u64) {
        match msg.payload {
            Some(Payload::GpioDrive(d)) => (d.pin, d.level, msg.sim_time_ns),
            other => panic!("expected a GpioDrive, got {other:?}"),
        }
    }

    /// Everything owed, polled a millisecond apart from `t0`.
    fn drain(k: &mut Ky040, t0: u64) -> Vec<(u32, bool)> {
        (0..).map(|i| t0 + i * 1_000_000).map_while(|t| k.poll(t)).map(|m| drive_of(m)).map(|(p, l, _)| (p, l)).collect()
    }

    /// IDF's `rotary_encoder` decode, both channels: A's edges gated by B
    /// (rising DECREASE, falling INCREASE, inverted while B is low) and B's
    /// gated by A (rising INCREASE, falling DECREASE, inverted while A is low).
    fn idf_count(edges: &[(u32, bool)]) -> i32 {
        let (mut a, mut b, mut n) = (true, true, 0);
        for &(pin, level) in edges {
            if pin == PIN_CLK {
                let step = if level { -1 } else { 1 };
                n += if b { step } else { -step };
                a = level;
            } else {
                let step = if level { 1 } else { -1 };
                n += if a { step } else { -step };
                b = level;
            }
        }
        n
    }

    #[test]
    fn a_clockwise_detent_is_four_edges_led_by_clk() {
        let mut k = wired();
        k.on_input("cw");
        let edges = drain(&mut k, 0);
        assert_eq!(edges, [(PIN_CLK, false), (PIN_DT, false), (PIN_CLK, true), (PIN_DT, true)]);
    }

    #[test]
    fn idfs_example_counts_clockwise_up_and_back_down() {
        let mut k = wired();
        k.on_input("cw");
        k.on_input("cw");
        assert_eq!(idf_count(&drain(&mut k, 0)), 8);
        k.on_input("ccw");
        let back = drain(&mut k, 10_000_000);
        assert_eq!(back[0], (PIN_DT, false), "counter-clockwise leads with DT");
        assert_eq!(idf_count(&back), -4);
    }

    #[test]
    fn every_edge_moves_one_line() {
        // Including across a reversal: both lines moving at once is the one
        // thing quadrature cannot mean.
        let mut k = wired();
        for turn in ["cw", "ccw", "ccw", "cw"] {
            k.on_input(turn);
        }
        let (mut clk, mut dt) = (true, true);
        for (pin, level) in drain(&mut k, 0) {
            let line = if pin == PIN_CLK { &mut clk } else { &mut dt };
            assert_ne!(*line, level, "an edge that moved nothing");
            *line = level;
        }
        assert_eq!((clk, dt), (true, true), "a detent ends with both lines high");
    }

    #[test]
    fn edges_are_spaced_on_the_emulated_clock() {
        let mut k = wired();
        k.on_input("cw");
        assert!(k.poll(1_000).is_some());
        assert!(k.poll(1_000).is_none(), "a second poll at the same time sends nothing");
        assert!(k.poll(1_000 + EDGE_GAP_NS - 1).is_none());
        assert_eq!(drive_of(k.poll(1_000 + EDGE_GAP_NS).unwrap()).2, 1_000 + EDGE_GAP_NS, "stamped with the poll's time");
    }

    #[test]
    fn a_clock_that_starts_again_does_not_stall_the_knob() {
        let mut k = wired();
        k.on_input("cw");
        assert!(k.poll(50_000_000).is_some());
        assert!(k.poll(1_000).is_some(), "the chip reset; the next edge goes on the new clock");
    }

    #[test]
    fn the_switch_pulls_its_line_low_and_goes_first() {
        let mut k = wired();
        k.on_input("cw");
        k.on_input("press");
        let (pin, level, _) = drive_of(k.poll(0).unwrap());
        assert_eq!((pin, level), (PIN_SW, false), "a press pulls SW low, ahead of the turn");
        assert_eq!(drive_of(k.poll(0).unwrap()).0, PIN_CLK, "then the turn's first edge");
        k.on_input("release");
        let (pin, level, _) = drive_of(k.poll(1_000_000).unwrap());
        assert_eq!((pin, level), (PIN_SW, true));
    }

    #[test]
    fn it_claims_the_lines_it_is_wired_to_and_no_other() {
        assert_eq!(wired().claims(), [drives(1 << PIN_CLK | 1 << PIN_DT | 1 << PIN_SW)]);
        let mut unwired = Ky040::default();
        assert!(unwired.claims().is_empty());
        unwired.on_input("cw");
        assert!(unwired.poll(0).is_none(), "an unwired knob has no line to move");
    }

    #[test]
    fn it_shows_its_position_and_its_switch() {
        let mut k = wired();
        k.on_input("ccw");
        k.on_input("ccw");
        k.on_input("press");
        let s = k.state().unwrap();
        assert_eq!((s.get("position"), s.get("pressed")), (Some("-2"), Some("true")));
    }
}
