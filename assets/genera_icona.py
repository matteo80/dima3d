"""Genera l'icona dell'applicazione (assets/icona.ico e assets/icona_256.png), senza librerie esterne.

Disegno: una scocca vista di lato appoggiata su uno skid arancione, su un quadrato arrotondato blu. Uso: python assets/genera_icona.py
"""
import math
import os
import struct
import zlib

SIZES = [16, 24, 32, 48, 64, 128, 256]
SAMPLES = 4  # sovracampionamento per lato: bordi morbidi

# Scocca vista di lato (senza ruote, con i passaruota) appoggiata su uno skid: la «dima» che
# posiziona i pezzi. Coordinate da 0 a 1, y verso il basso.
BODY = [
    (0.12, 0.60), (0.12, 0.52), (0.17, 0.48), (0.33, 0.45), (0.43, 0.31), (0.63, 0.30),
    (0.76, 0.44), (0.86, 0.47), (0.88, 0.52), (0.88, 0.60),
]
WINDOW = [(0.45, 0.345), (0.61, 0.335), (0.71, 0.445), (0.38, 0.45)]
ARCHES = [((0.30, 0.60), 0.085), ((0.71, 0.60), 0.085)]
SKID = [(0.10, 0.75, 0.90, 0.80), (0.15, 0.60, 0.20, 0.76), (0.475, 0.60, 0.525, 0.76), (0.81, 0.60, 0.86, 0.76)]

BODY_COLOR = (236, 243, 251)
WINDOW_COLOR = (112, 164, 226)
SKID_COLOR = (255, 152, 40)


def inside_polygon(x, y, poly):
    """Pari-dispari: va bene anche per poligoni non convessi."""
    inside = False
    for (ax, ay), (bx, by) in zip(poly, poly[1:] + poly[:1]):
        if (ay > y) != (by > y) and x < ax + (y - ay) * (bx - ax) / (by - ay):
            inside = not inside
    return inside


def inside_rounded_square(x, y, margin=0.04, radius=0.2):
    low, high = margin + radius, 1 - margin - radius
    dx = max(low - x, 0, x - high)
    dy = max(low - y, 0, y - high)
    return dx * dx + dy * dy <= radius * radius


def sample(x, y):
    if not inside_rounded_square(x, y):
        return (0, 0, 0, 0)
    for x0, y0, x1, y1 in SKID:
        if x0 <= x <= x1 and y0 <= y <= y1:
            return (*SKID_COLOR, 255)
    in_arch = any((x - cx) ** 2 + (y - cy) ** 2 <= r * r for (cx, cy), r in ARCHES)
    if inside_polygon(x, y, BODY) and not in_arch:
        return (*(WINDOW_COLOR if inside_polygon(x, y, WINDOW) else BODY_COLOR), 255)
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
