"""Readers for the FromSoftware container formats the extractor needs:
DCX (Oodle Kraken, via the game's own oo2core DLL) and BND4."""
import ctypes
import struct
from pathlib import Path

import paths

_oodle = None


def _oodle_decompress(data: bytes, raw_size: int) -> bytes:
    global _oodle
    if _oodle is None:
        _oodle = ctypes.WinDLL(str(paths.game_dir() / "oo2core_6_win64.dll"))
        _oodle.OodleLZ_Decompress.restype = ctypes.c_int64
        _oodle.OodleLZ_Decompress.argtypes = [
            ctypes.c_char_p, ctypes.c_int64, ctypes.c_char_p, ctypes.c_int64,
            ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_void_p, ctypes.c_int64,
            ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64, ctypes.c_int,
        ]
    out = ctypes.create_string_buffer(raw_size)
    n = _oodle.OodleLZ_Decompress(data, len(data), out, raw_size, 1, 0, 0, None, 0, None, None, None, 0, 3)
    if n != raw_size:
        raise ValueError(f"Oodle returned {n}, expected {raw_size}")
    return out.raw


def dcx(data: bytes) -> bytes:
    if data[:4] != b"DCX\0":
        return data
    assert data[0x18:0x1C] == b"DCS\0" and data[0x28:0x2C] == b"KRAK", data[0x28:0x2C]
    raw_size, comp_size = struct.unpack_from(">II", data, 0x1C)
    assert data[0x44:0x48] == b"DCA\0"
    return _oodle_decompress(data[0x4C:0x4C + comp_size], raw_size)


def _wstr(data: bytes, off: int) -> str:
    end = off
    while data[end:end + 2] != b"\0\0":
        end += 2
    return data[off:end].decode("utf-16le")


def bnd4(data: bytes) -> list[tuple[int, str, bytes]]:
    """Returns (id, name, bytes) for every file in a BND4."""
    assert data[:4] == b"BND4", data[:4]
    count, = struct.unpack_from("<i", data, 0x0C)
    entry_size, = struct.unpack_from("<q", data, 0x20)
    unicode = data[0x30]
    # The format flags are stored bit-reversed in little-endian archives.
    fmt = int(f"{data[0x31]:08b}"[::-1], 2)
    files = []
    for i in range(count):
        p = 0x40 + i * entry_size
        p += 8  # flags, padding, -1
        comp_size, = struct.unpack_from("<q", data, p); p += 8
        if fmt & 0x20:
            p += 8  # uncompressed size
        if fmt & 0x10:
            offset, = struct.unpack_from("<q", data, p); p += 8
        else:
            offset, = struct.unpack_from("<I", data, p); p += 4
        file_id = -1
        if fmt & 0x02:
            file_id, = struct.unpack_from("<i", data, p); p += 4
        name = ""
        if fmt & 0x0C:
            name_off, = struct.unpack_from("<I", data, p); p += 4
            name = _wstr(data, name_off) if unicode else data[name_off:data.index(b"\0", name_off)].decode("shift-jis")
        files.append((file_id, name, data[offset:offset + comp_size]))
    return files


def open_bnd(path) -> list[tuple[int, str, bytes]]:
    return bnd4(dcx(Path(path).read_bytes()))
