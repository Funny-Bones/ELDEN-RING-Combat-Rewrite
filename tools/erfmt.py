"""Readers for the FromSoftware container formats the extractor needs:
DCX (Oodle Kraken, via the game's own oo2core DLL) and BND4."""
import ctypes
import struct
from pathlib import Path

import paths

import os
import shutil
import sys
import subprocess
import atexit

_oodle = None
_server_proc = None


# Magic string, but honestly python.org isn't stripping this away ever -- also 10mb
EMBED_PYTHON_URL = "https://www.python.org/ftp/python/3.11.9/python-3.11.9-embed-amd64.zip"



def _ensure_wine_python():
    # == Linux/MacOS function ==

    # Check if we have an environment variable override
    env_path = os.environ.get("WINE_PYTHON")
    if env_path and (Path(env_path).exists() or shutil.which(env_path)):
        return env_path

    # Then, check if a pre-existing python.exe works directly under wine
    try:
        res = subprocess.run(
            ["wine", "python.exe", "-c", "import sys; sys.exit(0)"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=5,
        )
        if res.returncode == 0:
            return "python.exe"
    except Exception:
        pass

    # Sometimes other libraries use winepy, check if it's cached in ~/.cache/winepy
    cache_dir = Path.home() / ".cache" / "winepy"
    py_exe = cache_dir / "python.exe"
    if py_exe.exists():
        return str(py_exe)

    # If nothing else, automatically download and unpack Python embeddable zip into cache_dir
    print("Non-Windows platform detected: bootstrapping embedded Windows Python for Wine...")
    import urllib.request
    import zipfile

    cache_dir.mkdir(parents=True, exist_ok=True)
    zip_path = cache_dir / "python-embed.zip"
    try:
        urllib.request.urlretrieve(EMBED_PYTHON_URL, zip_path)
        with zipfile.ZipFile(zip_path, "r") as zf:
            zf.extractall(cache_dir)
        if zip_path.exists():
            zip_path.unlink()
        print(f"Bootstrapped embedded Windows Python to {cache_dir}")
        return str(py_exe)
    except Exception as exc:
        sys.exit(
            f"Failed to automatically download embedded Windows Python for Wine: {exc}\n"
            f"Please ensure you have an active internet connection, or install Python inside Wine and set WINE_PYTHON."
        )


def _get_server():
    # == Linux/MacOS function ==
    global _server_proc
    if _server_proc is None:
        if not shutil.which("wine"):
            sys.exit(
                "Error: 'wine' was not found on your PATH.\n"
                "On non-Windows platforms (Linux/macOS), Wine is required to interface with "
                "the game's oo2core_6_win64.dll.\nPlease install Wine or ensure it is in your PATH."
            )

        dll_path = str(paths.game_dir() / "oo2core_6_win64.dll")
        if not Path(dll_path).exists():
            sys.exit(f"Error: Oodle DLL not found at: {dll_path}")

        wine_py = _ensure_wine_python()
        wine_path = "Z:" + dll_path.replace("/", "\\")
        server_code = (
            "import sys, os, struct, ctypes\n"
            "dll_path = sys.argv[1]\n"
            "_oodle = ctypes.WinDLL(dll_path)\n"
            "_oodle.OodleLZ_Decompress.restype = ctypes.c_int64\n"
            "_oodle.OodleLZ_Decompress.argtypes = [\n"
            "    ctypes.c_char_p, ctypes.c_int64, ctypes.c_char_p, ctypes.c_int64,\n"
            "    ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_void_p, ctypes.c_int64,\n"
            "    ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64, ctypes.c_int,\n"
            "]\n"
            "if sys.platform == 'win32':\n"
            "    import msvcrt\n"
            "    msvcrt.setmode(sys.stdin.fileno(), os.O_BINARY)\n"
            "    msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)\n"
            "stdin = sys.stdin.buffer\n"
            "stdout = sys.stdout.buffer\n"
            "while True:\n"
            "    header = stdin.read(12)\n"
            "    if not header or len(header) < 12:\n"
            "        break\n"
            "    raw_size, comp_size = struct.unpack('>QI', header)\n"
            "    data = stdin.read(comp_size)\n"
            "    if len(data) < comp_size:\n"
            "        break\n"
            "    out = ctypes.create_string_buffer(raw_size)\n"
            "    n = _oodle.OodleLZ_Decompress(data, len(data), out, raw_size, 1, 0, 0, None, 0, None, None, None, 0, 3)\n"
            "    if n != raw_size:\n"
            "        stdout.write(struct.pack('>I', 0))\n"
            "        stdout.flush()\n"
            "    else:\n"
            "        stdout.write(struct.pack('>I', raw_size))\n"
            "        stdout.write(out.raw)\n"
            "        stdout.flush()\n"
        )

        try:
            _server_proc = subprocess.Popen(
                ["wine", wine_py, "-c", server_code, wine_path],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
        except Exception as exc:
            sys.exit(
                f"Failed to start Wine Python process with command 'wine {wine_py}': {exc}\n"
                f"Ensure Wine and Windows Python are available, or set WINE_PYTHON to your python.exe location."
            )
        atexit.register(lambda: _server_proc.terminate() if _server_proc else None)
    return _server_proc


def _oodle_decompress(data: bytes, raw_size: int) -> bytes:
    global _oodle
    # This is to allow our script here to work for mac/linux
    # Does require that the invoker has wine installed
    if sys.platform != "win32" and sys.platform != "win64":
        srv = _get_server()
        srv.stdin.write(struct.pack(">QI", raw_size, len(data)) + data)
        srv.stdin.flush()
        hdr = srv.stdout.read(4)
        if not hdr or len(hdr) < 4:
            raise ValueError("Oodle server communication error")
        out_len, = struct.unpack(">I", hdr)
        if out_len != raw_size:
            raise ValueError(f"Oodle decompression failed: got {out_len}, expected {raw_size}")
        return srv.stdout.read(out_len)

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
