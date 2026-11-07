"""Genera l'icona dell'applicazione (assets/icona.ico e assets/icona_256.png), senza librerie esterne.

Disegno: un cubo in assonometria su un quadrato arrotondato blu. Uso: python assets/genera_icona.py
"""
import math
import os
import struct
import zlib

SIZES = [16, 24, 32, 48, 64, 128, 256]
SAMPLES = 4  # sovracampionamento per lato: bordi morbidi

TOP = (0.50, 0.17)
CENTER = (0.50, 0.50)
LEFT_TOP = (0.21, 0.335)
RIGHT_TOP = (0.79, 0.335)
LEFT_BOTTOM = (0.21, 0.665)
RIGHT_BOTTOM = (0.79, 0.665)
BOTTOM = (0.50, 0.83)

FACES = [
    ([TOP, RIGHT_TOP, CENTER, LEFT_TOP], (233, 241, 250)),
    ([LEFT_TOP, CENTER, BOTTOM, LEFT_BOTTOM], (150, 192, 240)),
    ([RIGHT_TOP, RIGHT_BOTTOM, BOTTOM, CENTER], (86, 145, 220)),
]


def inside_polygon(x, y, poly):
    sign = 0
    for (ax, ay), (bx, by) in zip(poly, poly[1:] + poly[:1]):
        cross = (bx - ax) * (y - ay) - (by - ay) * (x - ax)
        if cross != 0:
            s = 1 if cross > 0 else -1
            if sign and s != sign:
                return False
            sign = s
    return True


def inside_rounded_square(x, y, margin=0.04, radius=0.2):
    low, high = margin + radius, 1 - margin - radius
    dx = max(low - x, 0, x - high)
    dy = max(low - y, 0, y - high)
    return dx * dx + dy * dy <= radius * radius


def sample(x, y):
    if not inside_rounded_square(x, y):
        return (0, 0, 0, 0)
    for poly, color in FACES:
        if inside_polygon(x, y, poly):
            return (*color, 255)
    t = y  # sfumatura verticale dello sfondo
    return (round(35 - 12 * t), round(92 - 34 * t), round(160 - 52 * t), 255)


def render(size):
    pixels = bytearray()
    for py in range(size):
        for px in range(size):
            r = g = b = a = 0
            for sy in range(SAMPLES):
                for sx in range(SAMPLES):
                    c = sample((px + (sx + 0.5) / SAMPLES) / size, (py + (sy + 0.5) / SAMPLES) / size)
                    r += c[0] * c[3]
                    g += c[1] * c[3]
                    b += c[2] * c[3]
                    a += c[3]
            if a:
                pixels += bytes((round(r / a), round(g / a), round(b / a), round(a / SAMPLES**2)))
            else:
                pixels += b"\0\0\0\0"
    return bytes(pixels)


def png(size, pixels):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    rows = b"".join(b"\0" + pixels[y * size * 4 : (y + 1) * size * 4] for y in range(size))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    images = [(size, png(size, render(size))) for size in SIZES]
    offset = 6 + 16 * len(images)
    header = struct.pack("<HHH", 0, 1, len(images))
    entries = b""
    for size, data in images:
        side = 0 if size == 256 else size
        entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    with open(os.path.join(here, "icona.ico"), "wb") as f:
        f.write(header + entries + b"".join(data for _, data in images))
    with open(os.path.join(here, "icona_256.png"), "wb") as f:
        f.write(images[-1][1])


if __name__ == "__main__":
    main()
