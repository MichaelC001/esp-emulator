#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Espressif Systems (Shanghai) CO LTD
#
# SPDX-License-Identifier: Apache-2.0

"""A DVP camera sensor on the co-simulation bus over --cosim-tcp, with no
protobuf library: it answers the sensor's SCCB (I2C) probe and sends frames
to the chip's camera receiver.

    esp-emu --chip esp32s31 --firmware app.bin --cosim-tcp 127.0.0.1:5594 &
    python3 tools/cosim-tcp-camera.py 5594 --pclk 54 --vsync 56 --de 57 \\
        --xclk 55 --data 46,47,48,49,50,51,52,53 --scl 1 --sda 0 \\
        --sensor ov2640 --fourcc YUYV --size 640x480 --fps 6

The pads are the ones the firmware's `esp_cam_ctlr_dvp_pin_config_t` and its
SCCB bus name. `--frames` is a JPEG file or a directory of them, sent in name
order and repeated; without it the sensor sends moving colour bars in
`--fourcc` at `--size`. What the firmware programs over SCCB is acknowledged
and read back, not interpreted: the format and size sent are the ones given
here, so give the ones the firmware asks its sensor for.
"""
import argparse
import os
import struct
import sys

from cosim_tcp import (accepted, cam_frame, cam_frame_pieces, connect, decode, dvp_claim, dvp_format, fourcc, i2c_claim,
                       i2c_reply, i2c_request, recv, register, send)

# name: (7-bit address, register address bytes, {register: value}). The
# identity registers each driver's probe reads.
SENSORS = {
    "ov2640": (0x30, 1, {0x0A: 0x26, 0x0B: 0x42, 0x1C: 0x7F, 0x1D: 0xA2}),
    "ov3640": (0x3C, 2, {0x300A: 0x36, 0x300B: 0x4C}),
    "ov5640": (0x3C, 2, {0x300A: 0x56, 0x300B: 0x40}),
}
LEAD = 2
BPP = {"RGBP": 2, "RGBR": 2, "UYVY": 2, "YUYV": 2, "YVYU": 2, "VYUY": 2, "GREY": 1}
BARS = [(255, 255, 255), (255, 255, 0), (0, 255, 255), (0, 255, 0), (255, 0, 255), (255, 0, 0), (0, 0, 255), (0, 0, 0)]


def num(text):
    return int(text, 0)


ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument("port", type=int)
ap.add_argument("--pclk", type=int, required=True, help="the pad the pixel clock is on")
ap.add_argument("--vsync", type=int, required=True)
ap.add_argument("--de", type=int, help="data enable (HREF)")
ap.add_argument("--xclk", type=int, help="the pad the chip's reference clock is on, if any")
ap.add_argument("--data", required=True, help="the eight data pads, D0 first, comma separated")
ap.add_argument("--scl", type=int, help="SCCB clock pad; with --sda, answer the sensor's I2C probe")
ap.add_argument("--sda", type=int)
ap.add_argument("--sensor", choices=sorted(SENSORS), default="ov2640",
                help="whose identity registers to answer with (default %(default)s)")
ap.add_argument("--addr", type=num, help="SCCB address, when not the sensor's usual one")
ap.add_argument("--reg", action="append", default=[], metavar="REG=VAL",
                help="another register's value, e.g. 0x300A=0x36 (repeatable)")
ap.add_argument("--frames", help="a JPEG file, or a directory of them")
ap.add_argument("--fourcc", default="RGBP", choices=sorted(BPP), help="the colour bars' format (default %(default)s)")
ap.add_argument("--size", default="320x240", help="the colour bars' size (default %(default)s)")
ap.add_argument("--fps", type=float, default=15.0, help="frames per second of emulated time (default %(default)s)")
args = ap.parse_args()

data_pins = [int(p) for p in args.data.split(",")]
if len(data_pins) != 8:
    ap.error("--data names eight pads")
addr, reg_bytes, regs = SENSORS[args.sensor]
regs = dict(regs)
addr = args.addr if args.addr is not None else addr
for item in args.reg:
    reg, _, val = item.partition("=")
    regs[num(reg)] = num(val)

jpegs = []
if args.frames:
    paths = [args.frames]
    if os.path.isdir(args.frames):
        paths = sorted(os.path.join(args.frames, n) for n in os.listdir(args.frames)
                       if n.lower().endswith((".jpg", ".jpeg")))
    jpegs = [open(p, "rb").read() for p in paths]
    if not jpegs:
        sys.exit(f"no JPEG in {args.frames}")
width, height = (int(v) for v in args.size.lower().split("x"))


def jpeg_size(data):
    """Width and height from a JPEG's frame header; (0, 0) if none is found."""
    i = 2
    while i + 9 < len(data) and data[i] == 0xFF:
        marker, (length,) = data[i + 1], struct.unpack(">H", data[i + 2:i + 4])
        if 0xC0 <= marker <= 0xCF and marker not in (0xC4, 0xC8, 0xCC):
            h, w = struct.unpack(">HH", data[i + 5:i + 9])
            return w, h
        i += 2 + length
    return 0, 0


def pixel(four, rgb, odd):
    r, g, b = rgb
    if four in ("RGBP", "RGBR"):
        v = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
        return struct.pack(">H" if four == "RGBP" else "<H", v)
    y = (66 * r + 129 * g + 25 * b + 128 >> 8) + 16
    if four == "GREY":
        return bytes([y])
    u = (-38 * r - 74 * g + 112 * b + 128 >> 8) + 128
    v = (112 * r - 94 * g - 18 * b + 128 >> 8) + 128
    c = (v if odd else u) if four in ("UYVY", "YUYV") else (u if odd else v)
    return bytes([c, y]) if four in ("UYVY", "VYUY") else bytes([y, c])


def bars(n):
    """Colour bars, shifted one bar per frame."""
    row = b"".join(pixel(args.fourcc, BARS[(x * 8 // width + n) % 8], x & 1) for x in range(width))
    return row * height


# Encoded once: a frame built while the emulator runs on arrives late.
if jpegs:
    frames = [cam_frame_pieces(j, *jpeg_size(j), fourcc("JPEG")) for j in jpegs]
else:
    frames = [cam_frame_pieces(bars(n), width, height, fourcc(args.fourcc)) for n in range(8)]


claims = [dvp_claim(args.pclk, args.vsync, data_pins, de_pin=args.de, xclk_pin=args.xclk)]
if args.scl is not None and args.sda is not None:
    claims.append(i2c_claim(addr, args.scl, args.sda))
s = connect(args.port)
send(s, register("camera.py", *claims))
topic, ack = recv(s)
ok, reason = accepted(ack)
if not ok:
    sys.exit(f"refused: {reason or ack!r}")

period = int(1_000_000_000 / args.fps)
capturing = False
session = 0         # the capture session the receiver last named
next_at = 0         # emulated time the next frame is due
sent = 0
pointer = 0         # the register a read starts at

while True:
    try:
        topic, payload = recv(s)
    except ConnectionError:                          # the emulator exited
        break
    if topic is None:
        break
    now = decode(payload).get(1, 0)                  # EmulatorMessage.sim_time_ns
    req = i2c_request(payload)
    if req is not None:
        cid, t = req
        if t.write:
            pointer = int.from_bytes(t.write[:reg_bytes], "big")
            for i, b in enumerate(t.write[reg_bytes:]):
                regs[pointer + i] = b
            if len(t.write) > reg_bytes:
                pointer += len(t.write) - reg_bytes
        out = bytes(regs.get(pointer + i, 0) for i in range(t.read_len))
        pointer += t.read_len
        send(s, i2c_reply(cid, out))
    f = dvp_format(payload)
    if f is not None:
        print(f"receiver: {'capturing' if f['capturing'] else 'stopped'}, XCLK {f['xclk_hz']} Hz")
        if f["capturing"] and not capturing:
            next_at = now + period
        capturing, session = f["capturing"], f["session"]
        continue                                     # frames follow the next heartbeat
    # Stay LEAD frames ahead: an emulator that is not held to wall-clock time
    # (--realtime) runs past a frame sent only when its time has come.
    while capturing and next_at <= now + LEAD * period:
        for msg in cam_frame(frames[sent % len(frames)], due_ns=next_at, session=session):
            send(s, msg)
        sent += 1
        next_at += period

print(f"sent {sent} frames")
