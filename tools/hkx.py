"""Pulls root motion out of a Havok 2018 tagfile animation without a full
type-system parse: the reference-frame object is found by its fixed layout
(up vector, forward vector, duration) and its sample array by the item table."""
import struct


def _sections(d, off, end, out):
    while off < end:
        size, = struct.unpack_from(">I", d, off)
        leaf, size = (size >> 30) & 1, size & 0x3FFFFFFF
        tag = d[off + 4:off + 8]
        out[tag] = (off + 8, off + size)
        if not leaf and size > 8:
            _sections(d, off + 8, off + size, out)
        if size == 0:
            break
        off += size


def root_motion(d: bytes):
    """Returns (duration_seconds, [(x, y, z, yaw)]) resampled to 30 fps, or None.

    Animations are authored at different rates (mostly 30 fps, some 60); the
    stored samples are one per authored frame, so they are resampled by time."""
    sec = {}
    _sections(d, 0, len(d), sec)
    data0 = sec[b"DATA"][0]
    a, b = sec[b"ITEM"]
    items = [struct.unpack_from("<III", d, o) for o in range(a, b, 12)]

    # The animation object says how many frames were authored.
    authored = None
    for _kind, off, count in items:
        p = data0 + off
        if count == 1 and p + 0x50 <= len(d) and struct.unpack_from("<i", d, p + 0x18)[0] == 3:
            frames, blocks, per_block = struct.unpack_from("<3i", d, p + 0x40)
            if 0 < frames < 100000 and 0 < blocks < 10000 and 1 < per_block <= 256:
                authored = frames
                break

    for _kind, off, count in items:
        if count != 1:
            continue
        p = data0 + off
        if p + 0x48 > len(d):
            continue
        # vtable/refcount/frameType header, then up (0,1,0,0) and forward (0,0,1,0).
        up = struct.unpack_from("<4f", d, p + 0x20)
        fwd = struct.unpack_from("<4f", d, p + 0x30)
        if up == (0.0, 1.0, 0.0, 0.0) and fwd == (0.0, 0.0, 1.0, 0.0):
            duration, = struct.unpack_from("<f", d, p + 0x40)
            # The samples are a vec4 array with one entry per authored frame.
            for _kind, soff, scount in reversed(items):
                if scount > 1 and (soff % 16) == 0 and scount == authored:
                    q = data0 + soff
                    if q + scount * 16 <= sec[b"DATA"][1]:
                        raw = [struct.unpack_from("<4f", d, q + i * 16) for i in range(scount)]
                        return duration, _resample(raw, duration)
    return None


def _resample(samples, duration):
    """One sample per 1/30 s across `duration`, interpolating the authored ones."""
    out = []
    steps = max(1, round(duration * 30.0))
    last = len(samples) - 1
    for i in range(steps + 1):
        x = min(i / steps, 1.0) * last
        lo = min(int(x), last)
        hi = min(lo + 1, last)
        t = x - lo
        out.append(tuple(a + (b - a) * t for a, b in zip(samples[lo], samples[hi])))
    return out
