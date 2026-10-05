"""Row access for Elden Ring .param files (64-bit offsets, fixed-size rows)."""
import struct
from pathlib import Path

PARAM_DIR = Path(r"C:\Users\Mateus\Desktop\er files\regulation-bin")


def rows(name: str) -> dict[int, bytes]:
    d = (PARAM_DIR / f"{name}.param").read_bytes()
    count, = struct.unpack_from("<H", d, 0x0A)
    entries = [struct.unpack_from("<iiqq", d, 0x40 + i * 24) for i in range(count)]
    strings, = struct.unpack_from("<I", d, 0x00)
    offsets = sorted(e[2] for e in entries)
    size = offsets[1] - offsets[0] if count > 1 else strings - offsets[0]
    return {rid: d[off:off + size] for rid, _pad, off, _name in entries}
