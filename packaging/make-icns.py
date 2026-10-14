#!/usr/bin/env python3
# Writes an .icns from PNG files, no macOS tools needed: the modern icns
# container is a list of (type, length, PNG bytes) chunks.
#
#   python3 packaging/make-icns.py OUT.icns 16=a.png 32=b.png 128=c.png 256=d.png 512=e.png
#
# Each PNG must already be its size; nothing is scaled here (packaging/brand-assets.sh does that).

import struct
import sys

TYPES = {
    16: b"icp4",
    32: b"icp5",
    64: b"icp6",
    128: b"ic07",
    256: b"ic08",
    512: b"ic09",
    1024: b"ic10",
}


def png_size(data: bytes) -> tuple[int, int]:
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        sys.exit("not a PNG")
    w, h = struct.unpack(">II", data[16:24])
    return w, h


def main() -> None:
    out = sys.argv[1]
    chunks = []
    for arg in sys.argv[2:]:
        size, path = arg.split("=", 1)
        size = int(size)
        with open(path, "rb") as f:
            data = f.read()
        if png_size(data) != (size, size):
            sys.exit(f"{path} is not {size}x{size}")
        chunks.append(TYPES[size] + struct.pack(">I", 8 + len(data)) + data)
    body = b"".join(chunks)
    with open(out, "wb") as f:
        f.write(b"icns" + struct.pack(">I", 8 + len(body)) + body)
    print(f"wrote {out} ({len(chunks)} sizes)")


if __name__ == "__main__":
    main()
