#!/usr/bin/env python3
"""Print mean/max RGB and coverage for an 8-bit non-interlaced PNG.

Pure stdlib (zlib) so it needs no Pillow; supports colour types 2 (RGB) and
6 (RGBA), which is what the VmodV screenshot writer produces.
"""
import struct
import sys
import zlib


def decode(path):
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path}: not a PNG")
    pos = 8
    idat = bytearray()
    width = height = depth = colour = interlace = None
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        chunk = data[pos + 8:pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width, height, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", chunk)
        elif kind == b"IDAT":
            idat += chunk
        elif kind == b"IEND":
            break
    if depth != 8 or interlace != 0 or colour not in (2, 6):
        raise SystemExit(
            f"{path}: unsupported PNG (depth={depth} colour={colour} interlace={interlace})"
        )
    channels = 4 if colour == 6 else 3
    stride = width * channels
    raw = zlib.decompress(bytes(idat))

    rows = []
    previous = bytearray(stride)
    offset = 0
    for _ in range(height):
        filter_type = raw[offset]
        offset += 1
        row = bytearray(raw[offset:offset + stride])
        offset += stride
        if filter_type == 0:
            pass
        elif filter_type == 1:  # Sub
            for i in range(channels, stride):
                row[i] = (row[i] + row[i - channels]) & 0xFF
        elif filter_type == 2:  # Up
            for i in range(stride):
                row[i] = (row[i] + previous[i]) & 0xFF
        elif filter_type == 3:  # Average
            for i in range(stride):
                left = row[i - channels] if i >= channels else 0
                row[i] = (row[i] + ((left + previous[i]) >> 1)) & 0xFF
        elif filter_type == 4:  # Paeth
            for i in range(stride):
                a = row[i - channels] if i >= channels else 0
                b = previous[i]
                c = previous[i - channels] if i >= channels else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                predictor = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                row[i] = (row[i] + predictor) & 0xFF
        else:
            raise SystemExit(f"{path}: bad filter type {filter_type}")
        rows.append(row)
        previous = row
    return width, height, channels, rows


def main(path):
    width, height, channels, rows = decode(path)
    total = width * height
    sums = [0, 0, 0]
    maxima = [0, 0, 0]
    above8 = 0
    nonzero = 0
    for row in rows:
        for pixel in range(0, len(row), channels):
            r, g, b = row[pixel], row[pixel + 1], row[pixel + 2]
            sums[0] += r
            sums[1] += g
            sums[2] += b
            if r > maxima[0]:
                maxima[0] = r
            if g > maxima[1]:
                maxima[1] = g
            if b > maxima[2]:
                maxima[2] = b
            if r > 8 or g > 8 or b > 8:
                above8 += 1
            if r or g or b:
                nonzero += 1
    mean = [value / total for value in sums]
    print(f"path={path}")
    print(f"size={width}x{height} channels={channels} pixels={total}")
    print(f"mean_rgb={mean[0]:.3f},{mean[1]:.3f},{mean[2]:.3f}")
    print(f"max_rgb={maxima[0]},{maxima[1]},{maxima[2]}")
    print(f"above8={above8} ({100.0 * above8 / total:.3f}%)")
    print(f"nonzero={nonzero} ({100.0 * nonzero / total:.3f}%)")
    if channels == 4:
        alpha_sum = alpha_max = alpha_nonzero = 0
        for row in rows:
            for pixel in range(3, len(row), channels):
                value = row[pixel]
                alpha_sum += value
                alpha_max = max(alpha_max, value)
                if value:
                    alpha_nonzero += 1
        print(f"alpha_mean={alpha_sum / total:.3f} alpha_max={alpha_max}")
        print(
            f"alpha_nonzero={alpha_nonzero} "
            f"({100.0 * alpha_nonzero / total:.3f}%)"
        )


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} <file.png>")
    main(sys.argv[1])
