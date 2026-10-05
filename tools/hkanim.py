"""Decoder for Havok 2018 skeletons and spline-compressed animations as stored
in Elden Ring's tagfiles. Objects are located by their fixed layouts and the
file's item table rather than by parsing the type section."""
import math
import re
import struct

from hkx import _sections


class Tagfile:
    def __init__(self, data: bytes):
        self.d = data
        sec = {}
        _sections(data, 0, len(data), sec)
        self.data0, self.data1 = sec[b"DATA"]
        a, b = sec[b"ITEM"]
        # (type index, offset into DATA, element count)
        self.items = [
            (t & 0xFFFFFF, off, count)
            for t, off, count in (struct.unpack_from("<III", data, o) for o in range(a, b, 12))
        ]

    def at(self, item):
        return self.data0 + self.items[item][1]


# --- Skeleton ---------------------------------------------------------------


def skeleton(data: bytes):
    """Returns (names, parents, reference pose) with one entry per bone.
    The pose is [(position xyz, rotation xyzw)] in parent space."""
    tag = Tagfile(data)
    # Three consecutive arrays of the same length: parent indices (int16),
    # bones (16 bytes) and the reference pose (48-byte transforms).
    for i in range(len(tag.items) - 2):
        (_, off_a, n), (_, off_b, nb), (_, off_c, nc) = tag.items[i : i + 3]
        if n > 8 and n == nb == nc and off_c - off_b == n * 16 and off_b - off_a >= n * 2:
            break
    else:
        raise ValueError("skeleton arrays not found")
    d = tag.d
    parents = list(struct.unpack_from("<%dh" % n, d, tag.data0 + off_a))
    pose = []
    for j in range(n):
        p = tag.data0 + off_c + j * 48
        tx, ty, tz, _tw, rx, ry, rz, rw = struct.unpack_from("<8f", d, p)
        pose.append(((tx, ty, tz), (rx, ry, rz, rw)))
    # Bone names are the string items that follow, one per bone, after the skeleton's own name.
    strings = []
    for _kind, off, count in tag.items[i + 3 :]:
        raw = d[tag.data0 + off : tag.data0 + off + count]
        if raw.endswith(b"\0") and re.fullmatch(rb"[\x20-\x7e]+\0", raw):
            strings.append(raw[:-1].decode())
    names = strings[1 : 1 + n]
    assert len(names) == n, (len(names), n)
    return names, parents, pose


# --- Spline-compressed animation --------------------------------------------


class Reader:
    def __init__(self, data, pos):
        self.d, self.p = data, pos

    def u8(self):
        v = self.d[self.p]
        self.p += 1
        return v

    def u16(self):
        (v,) = struct.unpack_from("<H", self.d, self.p)
        self.p += 2
        return v

    def f32(self):
        (v,) = struct.unpack_from("<f", self.d, self.p)
        self.p += 4
        return v

    def align(self, n, base):
        rem = (self.p - base) % n
        if rem:
            self.p += n - rem


def _quat40(r):
    raw = int.from_bytes(r.d[r.p : r.p + 5], "little")
    r.p += 5
    mask = (1 << 12) - 1
    half = mask >> 1
    scale = 0.000345436
    v = [((raw >> s) & mask) - half for s in (0, 12, 24)]
    v = [c * scale for c in v]
    shift = (raw >> 36) & 3
    w = math.sqrt(max(0.0, 1.0 - sum(c * c for c in v)))
    if (raw >> 38) & 1:
        w = -w
    v.insert(shift, w)
    return tuple(v)


def _quat48(r):
    x, y, z = struct.unpack_from("<3h", r.d, r.p)
    r.p += 6
    mask = (1 << 15) - 1
    half = mask >> 1
    scale = 0.000043161
    shift = ((y >> 14) & 2) | ((x >> 15) & 1)
    negative = (z >> 15) & 1
    v = [((c & mask) - half) * scale for c in (x, y, z)]
    w = math.sqrt(max(0.0, 1.0 - sum(c * c for c in v)))
    if negative:
        w = -w
    v.insert(shift, w)
    return tuple(v)


def _quat(r, kind):
    if kind == 1:
        return _quat40(r)
    if kind == 2:
        return _quat48(r)
    if kind == 5:
        v = struct.unpack_from("<4f", r.d, r.p)
        r.p += 16
        return v
    raise NotImplementedError("rotation quantization %d" % kind)


_QUAT_ALIGN = {0: 4, 1: 1, 2: 2, 3: 1, 4: 2, 5: 4}


def _find_span(n, degree, value, knots):
    if value >= knots[n + 1]:
        return n
    if value <= knots[0]:
        return degree
    low, high = degree, n + 1
    mid = (low + high) // 2
    while value < knots[mid] or value >= knots[mid + 1]:
        if value < knots[mid]:
            high = mid
        else:
            low = mid
        mid = (low + high) // 2
    return mid


def _nurbs(points, knots, degree, frame):
    """Evaluates a clamped B-spline; `points` are tuples of equal length."""
    n = len(points) - 1
    if n == 0:
        return points[0]
    span = _find_span(n, degree, frame, knots)
    basis = [0.0] * (degree + 1)
    basis[0] = 1.0
    for i in range(1, degree + 1):
        for j in range(i - 1, -1, -1):
            denom = knots[span + i - j] - knots[span - j]
            a = (frame - knots[span - j]) / denom if denom else 0.0
            tmp = basis[j] * a
            basis[j + 1] += basis[j] - tmp
            basis[j] = tmp
    dims = len(points[0])
    out = [0.0] * dims
    for i in range(degree + 1):
        point = points[span - i]
        for k in range(dims):
            out[k] += point[k] * basis[i]
    return out


class _Vec3Track:
    def __init__(self, r, base, types, quant, default):
        self.static = [default] * 3
        self.spline = None
        if types & 0x70:
            count = r.u16()
            self.degree = r.u8()
            self.knots = [r.u8() for _ in range(count + self.degree + 2)]
            r.align(4, base)
            bounds = [None] * 3
            for axis in range(3):
                if types & (0x10 << axis):
                    bounds[axis] = (r.f32(), r.f32())
                elif types & (1 << axis):
                    self.static[axis] = r.f32()
            self.spline = []
            for _ in range(count + 1):
                point = []
                for axis in range(3):
                    if bounds[axis] is None:
                        point.append(self.static[axis])
                        continue
                    lo, hi = bounds[axis]
                    ratio = r.u8() / 255.0 if quant == 0 else r.u16() / 65535.0
                    point.append(lo + (hi - lo) * ratio)
                self.spline.append(tuple(point))
        else:
            for axis in range(3):
                if types & (1 << axis):
                    self.static[axis] = r.f32()
        r.align(4, base)

    def at(self, frame):
        if self.spline is None:
            return tuple(self.static)
        return tuple(_nurbs(self.spline, self.knots, self.degree, frame))


class _QuatTrack:
    def __init__(self, r, base, types, quant):
        self.static = (0.0, 0.0, 0.0, 1.0)
        self.spline = None
        if types & 0xF0:
            count = r.u16()
            self.degree = r.u8()
            self.knots = [r.u8() for _ in range(count + self.degree + 2)]
            r.align(_QUAT_ALIGN[quant], base)
            self.spline = [_quat(r, quant) for _ in range(count + 1)]
        elif types & 0x0F:
            r.align(_QUAT_ALIGN[quant], base)
            self.static = _quat(r, quant)
        r.align(4, base)

    def at(self, frame):
        if self.spline is None:
            return self.static
        q = _nurbs(self.spline, self.knots, self.degree, frame)
        norm = math.sqrt(sum(c * c for c in q)) or 1.0
        return tuple(c / norm for c in q)


class Animation:
    """Per-frame local transforms for the animated tracks of one clip."""

    def __init__(self, data: bytes):
        tag = Tagfile(data)
        d = tag.d
        obj = None
        for index, (_kind, off, count) in enumerate(tag.items):
            p = tag.data0 + off
            if count == 1 and p + 0xB0 <= tag.data1 and struct.unpack_from("<i", d, p + 0x18)[0] == 3:
                frames, blocks, per_block = struct.unpack_from("<3i", d, p + 0x40)
                if 0 < frames < 100000 and 0 < blocks < 10000 and 1 < per_block <= 256:
                    obj = p
                    break
        if obj is None:
            raise ValueError("no spline-compressed animation in file")
        (self.duration,) = struct.unpack_from("<f", d, obj + 0x1C)
        (self.tracks,) = struct.unpack_from("<i", d, obj + 0x20)
        self.frames, blocks, per_block = struct.unpack_from("<3i", d, obj + 0x40)
        # Seconds per authored frame: most clips are 30 fps, some are 60.
        (self.frame_time,) = struct.unpack_from("<f", d, obj + 0x58)
        (block_item,) = struct.unpack_from("<q", d, obj + 0x60)
        (data_item,) = struct.unpack_from("<q", d, obj + 0xA0)
        offsets = struct.unpack_from("<%dI" % blocks, d, tag.at(block_item))
        blob = tag.at(data_item)

        # Track-to-bone map: an int16 array with one entry per track.
        self.bones = list(range(self.tracks))
        for _kind, off, count in tag.items:
            if count == self.tracks and count > 1:
                cand = struct.unpack_from("<%dh" % count, d, tag.data0 + off)
                if cand[0] >= 0 and all(b > a for a, b in zip(cand, cand[1:])) and cand[-1] < 1000:
                    self.bones = list(cand)
                    break

        self.per_block = per_block - 1
        self.blocks = []
        for block in range(blocks):
            base = blob + offsets[block]
            r = Reader(d, base)
            masks = [(r.u8(), r.u8(), r.u8(), r.u8()) for _ in range(self.tracks)]
            r.align(4, base)
            tracks = []
            for quant, pos_types, rot_types, scale_types in masks:
                pos = _Vec3Track(r, base, pos_types, quant & 3, 0.0)
                rot = _QuatTrack(r, base, rot_types, (quant >> 2) & 0xF)
                _scale = _Vec3Track(r, base, scale_types, (quant >> 6) & 3, 1.0)
                tracks.append((pos, rot))
            self.blocks.append(tracks)

    def frames_at_30fps(self):
        """How many 1/30 s steps the clip lasts."""
        return max(1, round(self.duration * 30.0))

    def sample_seconds(self, seconds: float):
        """[(position, rotation)] for every track at a time, whatever the authored rate."""
        if self.frame_time <= 0.0:
            return self.sample(0.0)
        return self.sample(seconds / self.frame_time)

    def sample(self, frame: float):
        """[(position, rotation)] for every track at authored `frame`."""
        frame = min(max(frame, 0.0), self.frames - 1)
        block = min(int(frame // self.per_block), len(self.blocks) - 1)
        local = frame - block * self.per_block
        return [(pos.at(local), rot.at(local)) for pos, rot in self.blocks[block]]
