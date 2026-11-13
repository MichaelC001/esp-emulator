#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Espressif Systems (Shanghai) CO LTD
#
# SPDX-License-Identifier: Apache-2.0

"""An audio codec on the co-simulation bus over --cosim-tcp, with no protobuf
library: a speaker that records what the chip plays to a WAV file, and a
microphone that plays it a tone.

    esp-emu --chip esp32s31 --firmware app.bin --cosim-tcp 127.0.0.1:5594 &
    python3 tools/cosim-tcp-i2s.py 5594 --dout 52 --din 40 --wav played.wav

`--dout` is the pad the chip transmits on and `--din` the pad it receives on,
as the firmware's `i2s_std_gpio_config_t` names them; give either or both.
The microphone waits for the receive format, then keeps 50 ms of a 440 Hz tone
queued ahead of emulated time, in the layout the firmware programmed.
"""
import argparse
import math
import struct
import sys
import wave

from cosim_tcp import (accepted, connect, decode, i2s_claim, i2s_format, i2s_in, i2s_out, recv,
                       register, send)

LEAD_NS = 50_000_000
TONE_HZ = 440

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument("port", type=int)
ap.add_argument("--dout", type=int, help="the pad the chip transmits on")
ap.add_argument("--din", type=int, help="the pad the chip receives on")
ap.add_argument("--wav", default="i2s-out.wav", help="where the speaker records (default %(default)s)")
args = ap.parse_args()
if args.dout is None and args.din is None:
    ap.error("name --dout, --din or both")

s = connect(args.port)
send(s, register("i2s.py", i2s_claim(args.dout, args.din)))
topic, ack = recv(s)
ok, reason = accepted(ack)
if not ok:
    sys.exit(f"refused: {reason or ack!r}")

out = None          # the WAV being recorded, opened at the transmit format
heard = 0
rx = None           # (rate, channels, bits) the receiver wants
rx_since = 0        # emulated time the receive format arrived
sent = 0            # frames supplied since then


def tone(first, frames, rate, channels, bits):
    """`frames` frames of the tone from frame `first`, every slot alike."""
    width = bits // 8
    peak = (1 << (bits - 1)) // 4
    fmt = {1: "<b", 2: "<h", 4: "<i"}[width]
    return b"".join(
        struct.pack(fmt, int(peak * math.sin(2 * math.pi * TONE_HZ * n / rate))) * channels
        for n in range(first, first + frames))


while True:
    try:
        topic, payload = recv(s)
    except ConnectionError:                          # the emulator exited
        break
    if topic is None:
        break
    now = decode(payload).get(1, 0)                  # EmulatorMessage.sim_time_ns
    f = i2s_format(payload)
    if f is not None:
        receive, rate, channels, bits = f
        print(f"{'receive' if receive else 'transmit'} format: {rate} Hz, {channels} slots, {bits} bits")
        if receive:
            rx, rx_since, sent = (rate, channels, bits), now, 0
        elif out is None:
            out = wave.open(args.wav, "wb")
            out.setnchannels(channels)
            out.setsampwidth(bits // 8)
            out.setframerate(rate)
    pcm = i2s_out(payload)
    if pcm and out is not None:
        out.writeframes(pcm)
        heard += len(pcm)
    if rx is not None and now >= rx_since:
        rate, channels, bits = rx
        due = (now - rx_since + LEAD_NS) * rate // 1_000_000_000
        if due > sent:
            send(s, i2s_in(tone(sent, due - sent, rate, channels, bits)))
            sent = due

if out is not None:
    out.close()
    print(f"recorded {heard} bytes to {args.wav}")
if rx is not None:
    print(f"supplied {sent} frames of a {TONE_HZ} Hz tone")
