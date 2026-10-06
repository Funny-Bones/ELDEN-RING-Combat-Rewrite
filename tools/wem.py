"""Converts Wwise Vorbis audio (.wem) into standard Ogg Vorbis, losslessly.

Wwise stores Vorbis without its headers: the setup header is stripped down
and its codebooks replaced by indices into an external codebook library, and
audio packets lose a few framing bits. This puts it all back, packet for
packet, without decoding or re-encoding anything.

A port of the relevant parts of ww2ogg by Adam Gashlin (BSD-3-Clause, see
tools/ww2ogg/COPYING), for the format Elden Ring uses: Wwise 2019, 0x42-byte
fmt chunk with the vorb data inside it, 2-byte packet headers, stripped
setup, external codebooks, modified packets. Granule positions are worked out
from the block sizes, so the result needs no further fixing up.
"""
import struct
from pathlib import Path

CODEBOOKS = Path(__file__).parent / "ww2ogg" / "packed_codebooks_aoTuV_603.bin"


def ilog(v):
    return v.bit_length()


def quantvals(entries, dimensions):
    """Tremor's _book_maptype1_quantvals."""
    bits = ilog(entries)
    vals = entries >> ((bits - 1) * (dimensions - 1) // dimensions)
    while True:
        acc, acc1 = vals ** dimensions, (vals + 1) ** dimensions
        if acc <= entries < acc1:
            return vals
        vals += -1 if acc > entries else 1


class BitReader:
    """Vorbis bit order: least significant bit of each byte first."""

    def __init__(self, data, start=0):
        self.value = int.from_bytes(data, "little")
        self.pos = start * 8
        self.end = len(data) * 8

    def read(self, n):
        if self.pos + n > self.end:
            raise ValueError("read past the end of a packet")
        v = (self.value >> self.pos) & ((1 << n) - 1)
        self.pos += n
        return v


class BitWriter:
    def __init__(self):
        self.value = 0
        self.pos = 0

    def write(self, v, n):
        self.value |= (v & ((1 << n) - 1)) << self.pos
        self.pos += n

    def bytes(self):
        return self.value.to_bytes((self.pos + 7) // 8, "little")


class Codebooks:
    def __init__(self, data):
        offsets_at, = struct.unpack_from("<I", data, len(data) - 4)
        count = (len(data) - offsets_at) // 4
        self.data = data
        self.offsets = struct.unpack_from(f"<{count}I", data, offsets_at)

    def rebuild(self, index, out):
        """Writes codebook `index` of the library to `out` in full Vorbis form."""
        if not 0 <= index < len(self.offsets) - 1:
            raise ValueError(f"no codebook {index}")
        start, end = self.offsets[index], self.offsets[index + 1]
        r = BitReader(self.data[start:end])
        dimensions, entries = r.read(4), r.read(14)
        out.write(0x564342, 24)
        out.write(dimensions, 16)
        out.write(entries, 24)
        ordered = r.read(1)
        out.write(ordered, 1)
        if ordered:
            out.write(r.read(5), 5)
            current = 0
            while current < entries:
                bits = ilog(entries - current)
                number = r.read(bits)
                out.write(number, bits)
                current += number
            if current > entries:
                raise ValueError("codebook entry count overrun")
        else:
            length_bits, sparse = r.read(3), r.read(1)
            if length_bits == 0 or length_bits > 5:
                raise ValueError("nonsense codeword length")
            out.write(sparse, 1)
            for _ in range(entries):
                present = 1
                if sparse:
                    present = r.read(1)
                    out.write(present, 1)
                if present:
                    out.write(r.read(length_bits), 5)
        lookup = r.read(1)
        out.write(lookup, 4)
        if lookup == 1:
            out.write(r.read(32), 32)
            out.write(r.read(32), 32)
            value_length = r.read(4)
            out.write(value_length, 4)
            out.write(r.read(1), 1)
            for _ in range(quantvals(entries, dimensions)):
                out.write(r.read(value_length + 1), value_length + 1)
        if r.pos // 8 + 1 != end - start:
            raise ValueError("codebook size mismatch")


_codebooks = None


def codebooks():
    global _codebooks
    if _codebooks is None:
        _codebooks = Codebooks(CODEBOOKS.read_bytes())
    return _codebooks


# --- Ogg pages -------------------------------------------------------------

def _crc_table():
    table = []
    for i in range(256):
        r = i << 24
        for _ in range(8):
            r = ((r << 1) ^ 0x04C11DB7) if r & 0x80000000 else (r << 1)
        table.append(r & 0xFFFFFFFF)
    return table


CRC = _crc_table()


def ogg_crc(data):
    crc = 0
    for b in data:
        crc = ((crc << 8) & 0xFFFFFFFF) ^ CRC[((crc >> 24) & 0xFF) ^ b]
    return crc


class OggWriter:
    """One packet per page, as ww2ogg writes them."""

    def __init__(self):
        self.out = bytearray()
        self.seqno = 0

    def page(self, packet, granule, last=False):
        if len(packet) > 255 * 255:
            raise ValueError("packet too big for one page")
        segments = [255] * (len(packet) // 255) + [len(packet) % 255]
        flags = (2 if self.seqno == 0 else 0) | (4 if last else 0)
        header = struct.pack("<4sBBqIIIB", b"OggS", 0, flags, granule, 1, self.seqno, 0, len(segments)) + bytes(segments)
        page = bytearray(header + packet)
        struct.pack_into("<I", page, 22, ogg_crc(page))
        self.out += page
        self.seqno += 1


# --- WEM -------------------------------------------------------------------

def _chunks(data):
    if data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError("not a little-endian RIFF WAVE")
    riff_size = struct.unpack_from("<I", data, 4)[0] + 8
    at, chunks = 12, {}
    while at + 8 <= riff_size:
        tag, size = data[at:at + 4], struct.unpack_from("<I", data, at + 4)[0]
        chunks[tag] = (at + 8, size)
        at += 8 + size
    return chunks


def info(data):
    """(channels, sample rate, sample count) of a Wwise Vorbis .wem."""
    fmt, _ = _chunks(data)[b"fmt "]
    codec, channels, rate = struct.unpack_from("<HHI", data, fmt)
    if codec != 0xFFFF:
        raise ValueError(f"codec {codec:#x} is not Wwise Vorbis")
    return channels, rate, struct.unpack_from("<I", data, fmt + 0x18)[0]


def to_ogg(data):
    """A Wwise Vorbis .wem as a standard Ogg Vorbis file."""
    chunks = _chunks(data)
    fmt, fmt_size = chunks[b"fmt "]
    data_at, data_size = chunks[b"data"]
    codec, channels, rate, avg_bytes = struct.unpack_from("<HHII", data, fmt)
    if codec != 0xFFFF:
        raise ValueError(f"codec {codec:#x} is not Wwise Vorbis")
    if b"vorb" in chunks:
        raise ValueError("separate vorb chunk: older Wwise layout, not handled")
    if fmt_size != 0x42:
        raise ValueError(f"unexpected fmt size {fmt_size:#x}")
    vorb = fmt + 0x18
    sample_count, mod_signal = struct.unpack_from("<II", data, vorb)
    # ww2ogg: these values mean standard packets; anything else, modified ones.
    mod_packets = mod_signal not in (0x4A, 0x4B, 0x69, 0x70)
    setup_offset, audio_offset = struct.unpack_from("<II", data, vorb + 0x10)
    _uid, bs0, bs1 = struct.unpack_from("<IBB", data, vorb + 0x24)

    ogg = OggWriter()

    # Identification header.
    w = BitWriter()
    w.write(1, 8)
    for c in b"vorbis":
        w.write(c, 8)
    w.write(0, 32)
    w.write(channels, 8)
    w.write(rate, 32)
    w.write(0, 32)
    w.write(avg_bytes * 8, 32)
    w.write(0, 32)
    w.write(bs0, 4)
    w.write(bs1, 4)
    w.write(1, 1)
    ogg.page(w.bytes(), 0)

    # Comment header.
    w = BitWriter()
    w.write(3, 8)
    for c in b"vorbis":
        w.write(c, 8)
    vendor = b"converted from Audiokinetic Wwise (ww2ogg port)"
    w.write(len(vendor), 32)
    for c in vendor:
        w.write(c, 8)
    w.write(0, 32)
    w.write(1, 1)
    ogg.page(w.bytes(), 0)

    # Setup header, rebuilt from the stripped one.
    p = data_at + setup_offset
    size, = struct.unpack_from("<H", data, p)
    r = BitReader(data[p + 2:p + 2 + size])
    w = BitWriter()
    w.write(5, 8)
    for c in b"vorbis":
        w.write(c, 8)
    book_count = r.read(8) + 1
    w.write(book_count - 1, 8)
    library = codebooks()
    for _ in range(book_count):
        library.rebuild(r.read(10), w)
    w.write(0, 6)  # time count - 1
    w.write(0, 16)  # dummy time value

    floor_count = r.read(6) + 1
    w.write(floor_count - 1, 6)
    for _ in range(floor_count):
        w.write(1, 16)  # floor type 1
        partitions = r.read(5)
        w.write(partitions, 5)
        classes = []
        for _ in range(partitions):
            c = r.read(4)
            w.write(c, 4)
            classes.append(c)
        dims = []
        for _ in range(max(classes, default=-1) + 1):
            d = r.read(3)
            w.write(d, 3)
            dims.append(d + 1)
            subclasses = r.read(2)
            w.write(subclasses, 2)
            if subclasses:
                master = r.read(8)
                w.write(master, 8)
                if master >= book_count:
                    raise ValueError("invalid floor masterbook")
            for _ in range(1 << subclasses):
                book = r.read(8)
                w.write(book, 8)
                if book - 1 >= book_count:
                    raise ValueError("invalid floor subclass book")
        w.write(r.read(2), 2)  # multiplier - 1
        rangebits = r.read(4)
        w.write(rangebits, 4)
        for c in classes:
            for _ in range(dims[c]):
                w.write(r.read(rangebits), rangebits)

    residue_count = r.read(6) + 1
    w.write(residue_count - 1, 6)
    for _ in range(residue_count):
        kind = r.read(2)
        if kind > 2:
            raise ValueError("invalid residue type")
        w.write(kind, 16)
        begin, end, partition = r.read(24), r.read(24), r.read(24)
        classifications = r.read(6) + 1
        classbook = r.read(8)
        w.write(begin, 24)
        w.write(end, 24)
        w.write(partition, 24)
        w.write(classifications - 1, 6)
        w.write(classbook, 8)
        cascade = []
        for _ in range(classifications):
            low = r.read(3)
            w.write(low, 3)
            flag = r.read(1)
            w.write(flag, 1)
            high = 0
            if flag:
                high = r.read(5)
                w.write(high, 5)
            cascade.append(high * 8 + low)
        for bits in cascade:
            for k in range(8):
                if bits & (1 << k):
                    book = r.read(8)
                    w.write(book, 8)
                    if book >= book_count:
                        raise ValueError("invalid residue book")

    mapping_count = r.read(6) + 1
    w.write(mapping_count - 1, 6)
    for _ in range(mapping_count):
        w.write(0, 16)  # mapping type 0
        submaps = 1
        flag = r.read(1)
        w.write(flag, 1)
        if flag:
            submaps = r.read(4) + 1
            w.write(submaps - 1, 4)
        polar = r.read(1)
        w.write(polar, 1)
        if polar:
            steps = r.read(8) + 1
            w.write(steps - 1, 8)
            bits = ilog(channels - 1)
            for _ in range(steps):
                magnitude, angle = r.read(bits), r.read(bits)
                w.write(magnitude, bits)
                w.write(angle, bits)
        reserved = r.read(2)
        w.write(reserved, 2)
        if reserved:
            raise ValueError("mapping reserved field nonzero")
        if submaps > 1:
            for _ in range(channels):
                w.write(r.read(4), 4)
        for _ in range(submaps):
            w.write(r.read(8), 8)  # time config
            w.write(r.read(8), 8)  # floor
            w.write(r.read(8), 8)  # residue

    mode_count = r.read(6) + 1
    w.write(mode_count - 1, 6)
    blockflags = []
    for _ in range(mode_count):
        flag = r.read(1)
        w.write(flag, 1)
        blockflags.append(flag)
        w.write(0, 16)  # window type
        w.write(0, 16)  # transform type
        w.write(r.read(8), 8)  # mapping
    w.write(1, 1)  # framing
    if (r.pos + 7) // 8 != size:
        raise ValueError("setup packet not read exactly")
    if p + 2 + size != data_at + audio_offset:
        raise ValueError("first audio packet doesn't follow the setup packet")
    ogg.page(w.bytes(), 0)

    # Audio packets.
    mode_bits = ilog(mode_count - 1)
    blocksizes = (1 << bs0, 1 << bs1)
    end = data_at + data_size
    at = data_at + audio_offset
    packets = []
    while at + 2 <= end:
        size, = struct.unpack_from("<H", data, at)
        packets.append(data[at + 2:at + 2 + size])
        at += 2 + size
    granule = 0
    prev_block = None
    prev_long = 0
    for i, payload in enumerate(packets):
        if mod_packets and payload:
            bits = int.from_bytes(payload, "little")
            mode = bits & ((1 << mode_bits) - 1)
            rest = bits >> mode_bits
            out = BitWriter()
            out.write(0, 1)  # audio packet
            out.write(mode, mode_bits)
            if blockflags[mode]:
                nxt = packets[i + 1] if i + 1 < len(packets) else b""
                next_long = blockflags[nxt[0] & ((1 << mode_bits) - 1)] if nxt else 0
                out.write(prev_long, 1)
                out.write(next_long, 1)
            out.write(rest, len(payload) * 8 - mode_bits)
            packet = out.bytes()
            prev_long = blockflags[mode]
            block = blocksizes[blockflags[mode]]
        else:
            packet = payload
            mode = (payload[0] >> 1) & ((1 << mode_bits) - 1) if payload else 0
            block = blocksizes[blockflags[mode]]
        # Each packet after the first completes (previous + current) / 4 samples.
        if prev_block is not None:
            granule += (prev_block + block) // 4
        prev_block = block
        last = i == len(packets) - 1
        ogg.page(packet, min(granule, sample_count) if last else granule, last)
    return bytes(ogg.out)
