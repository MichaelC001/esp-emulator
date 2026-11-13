# SG90: an example PWM part

A virtual TowerPro SG90 micro servo, written in Rust. The firmware drives it
with the stock `ledc` driver, or the `mcpwm` driver, as it would the real
servo. It is the part the [servo board](../../boards/servo/README.md) runs. To
start a part of your own, copy the simpler [push button](../pushbutton/README.md)
and follow its "Make your own part" steps; this one shows what a PWM part
adds: a `reads_pwm` claim, `ON_MESSAGE` and `on_pwm`.

## What it models

A hobby servo takes a pulse on its signal lead every 20 ms, and the width of
the pulse is the angle the horn turns to. The SG90 datasheet (TowerPro) is the
reference: a 1.5 ms pulse is the middle, about 2 ms all the way one way and
about 1 ms all the way the other, over about 180 degrees. Here 1 ms is 0
degrees, 1.5 ms 90 and 2 ms 180, in a straight line between; a pulse outside
1 to 2 ms holds the horn at the nearer end.

The part does not count edges. It claims its pin as PWM, and the emulator
tells it the period and the high time on the pin whenever they change, from
whichever block drives it: LEDC, or an MCPWM generator. A steady pulse is one
message however long it runs, and a hardware fade reaches it step by step with
no register write.

It holds position on a period from 3 ms to 25 ms (330 Hz to 40 Hz, the band
hobby servos take; the SG90 is specified at 20 ms). A pin with no pulses, held
low or high, or pulsed at a rate outside the band is not driven: the horn
stays where it was, as a real servo's does when its motor stops driving.

## What it does not model

- Speed. The datasheet gives 0.1 s per 60 degrees; here the horn is at the
  commanded angle at once.
- The dead band (10 us) and the travel past the 1 to 2 ms ends that real units
  have.
- Load, torque and stalling.

## What it shows

| Name | What it is |
|---|---|
| `angle` | Where the horn is, in degrees from one end, to a tenth. Absent until the first pulse: a servo never driven is wherever it was left. |
| `driven` | Whether the pin carries pulses the servo holds position on. |
| `pulse_us` | The last pulse width it was told, in whole microseconds. |

It has no interactions: a servo is driven by the chip, not by a person.

## Build it

You need Rust (<https://rustup.rs>) and its WebAssembly target:

```sh
rustup target add wasm32-unknown-unknown
```

Then, in this directory:

```sh
cargo build --release --target wasm32-unknown-unknown
```

The part is `target/wasm32-unknown-unknown/release/sg90.wasm`. `cargo test`
runs the servo's own tests on your computer, without the emulator.

## Put it on a board

A board file for `esp-emu --cosim-board`, with the servo's `SIG` on GPIO 4:

```json
{ "parts": { "servo": { "module": "sg90.wasm", "pins": { "SIG": 4 } } } }
```

A servo with no `SIG` (the name is upper case) claims nothing and is never
told anything.

## What is in it

| File | What it does |
|---|---|
| `Cargo.toml` | A `cdylib` crate: it builds to a `.wasm` module. It depends on [`esp-part`](../../../esp-part/README.md), the part SDK. |
| `src/lib.rs` | The servo (`Sg90`), as an `esp_part::Device` whose `on_pwm` takes the waveform on its pin, and the `export_part!` line that makes it a part: every `part_*` function of [`proto/PART_ABI.md`](../../../proto/PART_ABI.md), over the shared buffer. |
