# Encoder board: a rotary encoder counted by PCNT

ESP32-C6 firmware counting a KY-040 rotary encoder with the chip's pulse
counter, run by `esp-emu` with no hardware and no browser. The encoder is
[`examples/parts/ky040`](../../parts/ky040) compiled to WebAssembly;
`board.json` wires its `CLK` to GPIO 5, `DT` to GPIO 6 and the switch `SW` to
GPIO 7. The firmware uses the stock `esp_driver_pcnt`, set up as ESP-IDF's
`rotary_encoder` example sets it; it does not know the knob is virtual.

## Run it from a release

Download two files from the
[latest release](https://github.com/espressif/esp-emulator/releases/latest):
the emulator for your computer, `esp-emu-<version>-<platform>.tar.gz`, and the
examples kit, `esp-emu-<version>-examples.tar.gz`. Unpack both. The kit's
`encoder/` folder holds this board, ready to run:

| File | What it is |
|---|---|
| `board.json` | the board: one KY-040 on GPIO 5, 6 and 7 |
| `ky040.wasm` | the part |
| `pcnt_encoder-esp32c6.bin` | the firmware: [`test_apps/pcnt_encoder`](../../../test_apps/pcnt_encoder) for esp32c6 |

In that folder:

```sh
/path/to/esp-emu --chip esp32c6 --firmware pcnt_encoder-esp32c6.bin \
    --cosim-board board.json --control-tcp 127.0.0.1:5556
```

The firmware prints `ENCODER: ready clk=5 dt=6 sw=7` once the counter runs.
In a second terminal, connect to the emulator's control channel:

```sh
nc 127.0.0.1 5556
```

and type these lines, each followed by Enter:

```text
part encoder input cw
part encoder input cw
part encoder input press
part encoder input release
```

Each answers `ok`. A detent is four counts, so the firmware prints
`ENCODER: count=4`, then `ENCODER: count=8` and `ENCODER: watch=8` (a watch
point two detents out), then `ENCODER: switch=pressed` and
`ENCODER: switch=released`. `ccw` turns the other way and counts down. Ctrl-C
closes `nc`; the emulator keeps running.

## What you can send

One command a line, one answer a line:

| Command | Answer |
|---|---|
| `part list` | `ok encoder` |
| `part encoder input cw`, `ccw` | `ok`; one detent clockwise or counter-clockwise |
| `part encoder input press`, `release` | `ok`; the switch |
| `part encoder input cw after MS` | `ok`; handed over MS of emulated time from now |
| `part encoder state` | `ok position=2 pressed=false` |
| `part clock` | `ok MS`: emulated time |
| `part pending` | `ok N`: inputs not handed over yet |

Anything else answers `err: ...`. The input name goes to the part as typed,
and the part ignores a name it does not have, so a misspelt `cw` answers `ok`
and does nothing. `--cosim-log events.jsonl` records every message between
the chip and the part, one JSON line each: here, the edges the knob put on
`CLK` and `DT`, in order, each stamped with emulated time.

## Build the pieces yourself

- The part: [`examples/parts/ky040`](../../parts/ky040) says how; the build
  gives `ky040.wasm`.
- The firmware, with ESP-IDF: in `test_apps/pcnt_encoder`,
  `idf.py set-target esp32c6 build merge-bin -o merged_flash.bin` gives
  `build/merged_flash.bin`.

Put both next to `board.json`, the firmware as `pcnt_encoder-esp32c6.bin`,
and run as above. The firmware builds for every chip with a pulse counter
(ESP32-C5, C6, H2, P4, S3 and S31; the ESP32-C3 has none) on the same pins;
pass that chip to `--chip`.
