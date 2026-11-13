# Servo board: a PWM part in the emulator

ESP32-C3 firmware driving an SG90 micro servo with the stock `ledc` driver,
run by `esp-emu` with no hardware and no browser. The servo is
[`examples/parts/sg90`](../../parts/sg90) compiled to WebAssembly;
`board.json` wires its signal lead, `SIG`, to GPIO 4. The servo is told the
period and the pulse width on its pin whenever they change, and turns its
horn to the angle the pulse commands: 1 ms is 0 degrees, 1.5 ms 90, 2 ms 180.
The firmware does not know the servo is virtual.

## Run it from a release

Download two files from the
[latest release](https://github.com/espressif/esp-emulator/releases/latest):
the emulator for your computer, `esp-emu-<version>-<platform>.tar.gz`, and the
examples kit, `esp-emu-<version>-examples.tar.gz`. Unpack both. The kit's
`servo/` folder holds this board, ready to run:

| File | What it is |
|---|---|
| `board.json` | the board: one SG90 with its `SIG` on GPIO 4 |
| `sg90.wasm` | the part |
| `pwm_servo-esp32c3.bin` | the firmware: [`test_apps/pwm_servo`](../../../test_apps/pwm_servo) for esp32c3 |

In that folder:

```sh
/path/to/esp-emu --chip esp32c3 --firmware pwm_servo-esp32c3.bin \
    --cosim-board board.json --control-tcp 127.0.0.1:5556
```

The firmware sends pulses of 1000, 1500 and 2000 us, each printed as
`PWM_SERVO: ledc pulse_us=1000`, then has LEDC fade the pulse from 1000 to
2000 us over one second, which turns the servo with no register write while
it runs, then sets 1500 us and prints `PWM_SERVO: done`. In a second
terminal, connect to the emulator's control channel:

```sh
nc 127.0.0.1 5556
```

and type this line, followed by Enter:

```text
part servo state
```

It answers `ok angle=90.0 driven=true pulse_us=1500`: the horn at the middle.
Ctrl-C closes `nc`; the emulator keeps running.

## What you can send

| Command | Answer |
|---|---|
| `part list` | `ok servo` |
| `part servo state` | `ok angle=A driven=B pulse_us=P`: where the horn is, whether the pin carries pulses it holds position on, the last pulse width |
| `part clock` | `ok MS`: emulated time |

`--cosim-log events.jsonl` records every message between the chip and the
part, one JSON line each: the waveform the servo was told, as `PwmState` with
its `period_ns` and `high_ns`.

## Build the pieces yourself

- The part: [`examples/parts/sg90`](../../parts/sg90) says how; the build
  gives `sg90.wasm`.
- The firmware, with ESP-IDF: in `test_apps/pwm_servo`,
  `idf.py set-target esp32c3 build merge-bin -o merged_flash.bin` gives
  `build/merged_flash.bin`.

Put both next to `board.json`, the firmware as `pwm_servo-esp32c3.bin`, and
run as above. Built for a chip with MCPWM (every one but the C3), the
firmware also drives a second servo through the MCPWM driver on GPIO 5; a
second placement, `"servo_mcpwm": { "module": "sg90.wasm", "pins": { "SIG": 5
} }`, puts one there. To write a part of your own, see
[docs/guides/PARTS.md](../../../docs/guides/PARTS.md).
