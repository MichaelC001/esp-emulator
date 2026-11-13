# CO₂ board: a serial part in the emulator

ESP32-C3 firmware reading an MH-Z19B CO₂ sensor over a UART, run by `esp-emu`
with no hardware and no browser. The sensor is
[`examples/parts/mhz19b`](../../parts/mhz19b) compiled to WebAssembly;
`board.json` wires its `TX` to GPIO 5 and its `RX` to GPIO 4, so the chip
transmits to it on GPIO 4 and hears it on GPIO 5. The firmware uses the stock
`uart` driver on UART1; it does not know the sensor is virtual.

## Run it from a release

Download two files from the
[latest release](https://github.com/espressif/esp-emulator/releases/latest):
the emulator for your computer, `esp-emu-<version>-<platform>.tar.gz`, and the
examples kit, `esp-emu-<version>-examples.tar.gz`. Unpack both. The kit's
`co2/` folder holds this board, ready to run:

| File | What it is |
|---|---|
| `board.json` | the board: one MH-Z19B with its `TX` on GPIO 5 and its `RX` on GPIO 4 |
| `mhz19b.wasm` | the part |
| `uart_mhz19b-esp32c3.bin` | the firmware: [`test_apps/uart_mhz19b`](../../../test_apps/uart_mhz19b) for esp32c3 |

In that folder:

```sh
/path/to/esp-emu --chip esp32c3 --firmware uart_mhz19b-esp32c3.bin \
    --cosim-board board.json --control-tcp 127.0.0.1:5556
```

The firmware sends the sensor its read command three times and prints each
answer, `MHZ19B: port=1 read=0 co2=400 crc=ok`. It prints
`UART_MHZ19B: done`, then a reading every second: `CO2: 400`. In a second
terminal, connect to the emulator's control channel:

```sh
nc 127.0.0.1 5556
```

and type this line, followed by Enter:

```text
part co2 input set:co2=1200
```

It answers `ok`, and the next reading is `CO2: 1200`. `more` and `less` move
it by 100 ppm. Ctrl-C closes `nc`; the emulator keeps running.

## What you can send

One command a line, one answer a line:

| Command | Answer |
|---|---|
| `part list` | `ok co2` |
| `part co2 input more`, `less` | `ok` |
| `part co2 input more after MS` | `ok`; handed over MS of emulated time from now |
| `part co2 input set:co2=1200` | `ok`; that value, in one step |
| `part clock` | `ok MS`: emulated time |
| `part pending` | `ok N`: inputs not handed over yet |

Anything else answers `err: ...`. The input name goes to the part as typed,
and the part ignores a name it does not have, so a misspelt `more` answers
`ok` and does nothing. `--cosim-log events.jsonl` records every byte between
the chip and the part, one JSON line a message: what the chip transmitted, as
`UartTx` with its port, and what the sensor answered, as `UartRx`.

## Build the pieces yourself

- The part: [`examples/parts/mhz19b`](../../parts/mhz19b) says how; the build
  gives `mhz19b.wasm`.
- The firmware, with ESP-IDF: in `test_apps/uart_mhz19b`,
  `idf.py set-target esp32c3 build merge-bin -o merged_flash.bin` gives
  `build/merged_flash.bin`.

Put both next to `board.json`, the firmware as `uart_mhz19b-esp32c3.bin`, and
run as above. Built for an ESP32-S3 the firmware puts the line on GPIO 17 (TX)
and 18 (RX), UART1's own pads, and for an ESP32-P4 on GPIO 32 and 33, so a
board for those wires the sensor's `RX` and `TX` there. To write a part of
your own, see [docs/guides/PARTS.md](../../../docs/guides/PARTS.md).
