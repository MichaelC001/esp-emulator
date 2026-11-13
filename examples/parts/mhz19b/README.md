# MH-Z19B: an example serial part

A virtual Winsen MH-Z19B infrared CO₂ sensor, written in Rust. The firmware
talks to it over a UART with the stock `uart` driver, as it would to the real
sensor. It is the part the [CO₂ board](../../boards/co2/README.md) runs. To
start a part of your own, copy the simpler [push button](../pushbutton/README.md)
and follow its "Make your own part" steps; this one shows what a serial part
adds: a serial claim, `ON_MESSAGE`, `on_uart` and `transmit`.

## What it models

The sensor is on a serial line at 9600 baud, 8N1. The firmware sends it a
nine-byte command and, for a read, it answers with nine bytes. The "MH-Z19B
Intelligent Infrared Gas Module User's Manual" (Winsen, V1.0) is the
reference:

| | Bytes |
|---|---|
| Read the concentration | `FF 01 86 00 00 00 00 00 79` |
| The answer | `FF 86 HH LL 00 00 00 00 CS` |

The concentration in ppm is `HH * 256 + LL`. `CS`, the last byte of every
frame, is the two's complement of the sum of bytes 1 to 7. Bytes 4 to 7 of the
answer are not defined by the manual and read 0 here.

A command may arrive in pieces, and several may arrive at once: the sensor
collects bytes until a frame is whole. Bytes before a frame's `FF` are
skipped, and a frame whose checksum is wrong is not answered. The other
commands of the manual, zero point and span calibration, automatic baseline
correction and the detection range, have no answer and change nothing here.

The part claims the serial line on its `TX` and `RX` pins. Which UART that is,
the emulator works out from the firmware's pin setup.

## What it does not model

- Warming up. The real sensor needs three minutes after power-on before its
  readings mean anything. Here the first reading is good.
- Measurement time and noise. A reading is the concentration now, exactly.
- Calibration, automatic baseline correction and the detection range: the
  commands are taken and do nothing.
- The PWM output and the analog output. Only the serial line is modelled.
- The baud rate. The sensor answers at whatever rate the firmware set.

## Interactions

| Name | What it does |
|---|---|
| `more`, `less` | Concentration up or down 100 ppm. |
| `set:co2=<ppm>` | That value, in one step. The value is a plain decimal, such as `1200`. |

It starts at 400 ppm, fresh outdoor air, and stays between 0 and 5000 ppm, the
range of the manual's specification table.

## Build it

You need Rust (<https://rustup.rs>) and its WebAssembly target:

```sh
rustup target add wasm32-unknown-unknown
```

Then, in this directory:

```sh
cargo build --release --target wasm32-unknown-unknown
```

The part is `target/wasm32-unknown-unknown/release/mhz19b.wasm`. `cargo test`
runs the sensor's own tests on your computer, without the emulator.

## Put it on a board

A board file for `esp-emu --cosim-board`, with the sensor's `TX` on GPIO 5 and
its `RX` on GPIO 4, so the chip transmits on GPIO 4 and receives on GPIO 5:

```json
{ "parts": { "co2": { "module": "mhz19b.wasm", "pins": { "TX": 5, "RX": 4 } } } }
```

A sensor with no `TX` or no `RX` (the names are upper case) is refused when the
board loads. Pins the firmware puts on two different UARTs are reported once
it does, and a sensor on UART0 while UART0 is the console is not wired to it
until the console moves.

## What is in it

| File | What it does |
|---|---|
| `Cargo.toml` | A `cdylib` crate: it builds to a `.wasm` module. It depends on [`esp-part`](../../../esp-part/README.md), the part SDK. |
| `src/lib.rs` | The sensor (`Mhz19b`), as an `esp_part::Device` whose `on_uart` takes the firmware's commands and whose `poll` sends the answers, and the `export_part!` line that makes it a part: every `part_*` function of [`proto/PART_ABI.md`](../../../proto/PART_ABI.md), over the shared buffer. |
