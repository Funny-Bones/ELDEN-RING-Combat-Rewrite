"""Minimal reader for Elden Ring's TAE3 animation-event files."""
import struct
from dataclasses import dataclass, field


@dataclass
class Event:
    type: int
    start: float
    end: float
    params: bytes

    def s32(self, i): return struct.unpack_from("<i", self.params, i * 4)[0]
    def f32(self, i): return struct.unpack_from("<f", self.params, i * 4)[0]


@dataclass
class Anim:
    id: int
    events: list = field(default_factory=list)
    # Another animation in the same file this one takes its events from, if any.
    import_from: int | None = None
    # Full id (category * 1000000 + id) of the animation whose HKX this one plays, if not its own.
    hkx_from: int | None = None


def parse(data: bytes) -> dict[int, Anim]:
    assert data[:4] == b"TAE " and data[7] == 0xFF, "expected 64-bit little-endian TAE"
    count, = struct.unpack_from("<i", data, 0x54)
    table, = struct.unpack_from("<q", data, 0x58)
    q = lambda off: struct.unpack_from("<q", data, off)[0]

    anims = {}
    headers = []
    for i in range(count):
        anim_id, off = struct.unpack_from("<qq", data, table + i * 16)
        ev_off, _grp_off, _times_off, file_off = struct.unpack_from("<qqqq", data, off)
        ev_count, = struct.unpack_from("<i", data, off + 32)
        anim = Anim(anim_id)
        kind, body = struct.unpack_from("<qq", data, file_off)
        if kind == 1:
            ref, = struct.unpack_from("<i", data, body)
            anim.import_from = ref
        else:
            # Full id of the animation whose HKX this one plays: category * 1000000 + id.
            imports_hkx = data[body + 9]
            ref, = struct.unpack_from("<i", data, body + 12)
            if imports_hkx and ref >= 0:
                anim.hkx_from = ref
        anims[anim_id] = anim
        headers.append((anim, ev_off, ev_count))

    # Parameter blocks have no stored length; each runs up to the next structure.
    starts = []
    raw = []
    for anim, ev_off, ev_count in headers:
        for j in range(ev_count):
            s_off, e_off, d_off = struct.unpack_from("<qqq", data, ev_off + j * 24)
            ev_type, p_off = struct.unpack_from("<qq", data, d_off)
            raw.append((anim, ev_type, s_off, e_off, p_off))
            starts.append(p_off)
            starts.append(d_off)
        if ev_count:
            starts.append(ev_off)
    bounds = sorted(set(starts))
    import bisect
    for anim, ev_type, s_off, e_off, p_off in raw:
        nxt = bisect.bisect_right(bounds, p_off)
        end = bounds[nxt] if nxt < len(bounds) else p_off + 16
        start, = struct.unpack_from("<f", data, s_off)
        stop, = struct.unpack_from("<f", data, e_off)
        anim.events.append(Event(ev_type, start, stop, data[p_off:min(end, p_off + 64)]))
    return anims
