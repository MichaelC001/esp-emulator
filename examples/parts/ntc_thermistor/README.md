# NTC thermistor: an example analog part

A virtual thermistor module, written in Rust: the divider many temperature
boards carry, read by the firmware with the stock `adc_oneshot` driver. It is
the second part of the [analog board](../../boards/analog/README.md); the
[potentiometer](../potentiometer/README.md) beside it is the simpler one.

## What it models

A 10 kΩ fixed resistor from the 3.3 V rail to the output and a 10 kΩ (at
25 °C) NTC thermistor, B 3950, from the output to ground. The output is on an
ADC pad:

```text
R(T) = 10 kΩ · exp(3950 · (1/T − 1/298.15 K))
V    = 3300 mV · R(T) / (10 kΩ + R(T))
```

1650 mV at 25 °C, 871 mV at 50 °C, 2543 mV at 0 °C: the output falls as the
thermistor warms. The part claims the pad and holds the output's voltage
there whenever the temperature moves.

## What it does not model

- Self-heating and the thermistor's thermal time constant: the output follows
  a change at once.
- Tolerance of the resistors and of B.

## Interactions

| Name | What it does |
|---|---|
| `warmer`, `cooler` | 1 °C up or down. |
| `set:temperature=<°C>` | That temperature, in one step. The value is a plain decimal, such as `37.5`. |

It starts at 25 °C and stays between −40 °C and 125 °C.

## Build it

As the [potentiometer](../potentiometer/README.md#build-it); the part is
`target/wasm32-unknown-unknown/release/ntc_thermistor.wasm`.

## Put it on a board

```json
{ "parts": { "ntc": { "module": "ntc_thermistor.wasm", "pins": { "OUT": 3 } } } }
```

The pin must be an ADC pad of the chip, as for the potentiometer.

## What is in it

| File | What it does |
|---|---|
| `Cargo.toml` | A `cdylib` crate on [`esp-part`](../../../esp-part/README.md). |
| `src/lib.rs` | The module (`Thermistor`), as an `esp_part::Device` whose `poll` holds the divider's output on its pad, and the `export_part!` line. |
