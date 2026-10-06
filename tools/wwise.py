"""Reader for Elden Ring's Wwise sound banks (.bnk, Wwise 2019.2, bank version 135).

Only as much as playing an event needs: which sounds an event's Play actions
end up at, and which media (.wem) each sound plays. Containers aren't parsed
in full; every sound and container records its parent, so a container's
children are found by turning that around. Switch containers do get their
switch list read, to pick the default switch's children.
"""
import struct
from dataclasses import dataclass, field

SOUND, ACTION, EVENT, RANSEQ, SWITCH, ACTOR_MIXER, LAYER = 2, 3, 4, 5, 6, 7, 9
ACTION_PLAY = 0x0403
PROP_VOLUME = 0x00  # dB


def fnv1(name):
    """Wwise's hash of an object name: 32-bit FNV-1 of the lower-cased name."""
    h = 2166136261
    for b in name.lower().encode():
        h = (h * 16777619) & 0xFFFFFFFF
        h ^= b
    return h


@dataclass
class Node:
    kind: int
    id: int
    body: bytes
    parent: int = 0
    children: list = field(default_factory=list)
    media: int = 0  # sounds: the .wem they play
    stream: int = 0  # sounds: 0 in a bank, 1 prefetched, 2 streamed
    volume: float = 0.0  # dB, relative to the parent


def _base_params(body, at):
    """(DirectParentID, volume in dB) from the NodeBaseParams starting at `at`."""
    fx = body[at + 1]
    at += 2 + (1 + 7 * fx if fx else 0)
    at += 1  # bOverrideAttachmentParams
    _bus, parent = struct.unpack_from("<II", body, at)
    at += 8 + 1  # byBitVector
    count = body[at]
    volume = 0.0
    for i, prop in enumerate(body[at + 1:at + 1 + count]):
        if prop == PROP_VOLUME:
            volume, = struct.unpack_from("<f", body, at + 1 + count + 4 * i)
    return parent, volume


class Bank:
    def __init__(self, data):
        self.objects = {}
        self.media = {}  # embedded .wem: id -> bytes
        at = 0
        didx = None
        data_at = None
        while at + 8 <= len(data):
            tag, size = data[at:at + 4], struct.unpack_from("<I", data, at + 4)[0]
            body = at + 8
            if tag == b"DIDX":
                didx = (body, size)
            elif tag == b"DATA":
                data_at = body
            elif tag == b"HIRC":
                count, = struct.unpack_from("<I", data, body)
                q = body + 4
                for _ in range(count):
                    kind = data[q]
                    length, oid = struct.unpack_from("<II", data, q + 1)
                    self.objects[oid] = Node(kind, oid, data[q + 9:q + 5 + length])
                    q += 5 + length
            at = body + size
        if didx and data_at is not None:
            p, size = didx
            for i in range(size // 12):
                mid, off, length = struct.unpack_from("<III", data, p + i * 12)
                self.media[mid] = data[data_at + off:data_at + off + length]
        self._link()

    def _link(self):
        for node in self.objects.values():
            b = node.body
            try:
                if node.kind == SOUND:
                    plugin, stream, media, _size, _bits = struct.unpack_from("<IBIIB", b, 0)
                    at = 14
                    if plugin & 0xF == 2:  # source plugin: its parameters follow
                        psize, = struct.unpack_from("<I", b, at)
                        at += 4 + psize
                    node.media, node.stream = media, stream
                    if plugin & 0xF != 1:
                        node.media = 0  # a generator (silence, tone), not a file
                    node.parent, node.volume = _base_params(b, at)
                elif node.kind in (RANSEQ, SWITCH, LAYER, ACTOR_MIXER):
                    node.parent, node.volume = _base_params(b, 0)
            except struct.error:
                continue
        for node in self.objects.values():
            if node.parent in self.objects:
                self.objects[node.parent].children.append(node.id)

    def event(self, name):
        return self.objects.get(fnv1(name))

    def play_targets(self, event):
        """Objects the event's Play actions start."""
        count = event.body[0]
        targets = []
        for i in range(count):
            action = self.objects.get(struct.unpack_from("<I", event.body, 1 + 4 * i)[0])
            if action and action.kind == ACTION:
                kind, target = struct.unpack_from("<HI", action.body, 0)
                if kind == ACTION_PLAY:
                    targets.append(target)
        return targets

    def _switch_items(self, node):
        """Children a switch container plays for its default switch."""
        b = node.body
        children = sorted(node.children)
        # The children list sits just before the switch list: a count, then the IDs in order.
        needle = struct.pack("<I", len(children)) + b"".join(struct.pack("<I", c) for c in children)
        at = b.find(needle)
        if at < 9 or not children:
            return children[:1]
        default, = struct.unpack_from("<I", b, at - 5)
        q = at + len(needle)
        groups, = struct.unpack_from("<I", b, q)
        q += 4
        found = []
        for _ in range(groups):
            switch, items = struct.unpack_from("<II", b, q)
            ids = list(struct.unpack_from(f"<{items}I", b, q + 8))
            q += 8 + 4 * items
            if switch == default:
                return ids
            found.append(ids)
        return found[0] if found else children[:1]

    def loudness(self, oid):
        """A node's volume in dB: its own plus every ancestor's, as Wwise adds them up."""
        db, seen = 0.0, set()
        while oid in self.objects and oid not in seen:
            seen.add(oid)
            node = self.objects[oid]
            db += node.volume
            oid = node.parent
        return db

    def tree(self, oid, depth=0):
        """What playing object `oid` plays, as a tree:
        ("media", id, dB) | ("random", [tree...]) | ("all", [tree...])."""
        node = self.objects.get(oid)
        if node is None or depth > 16:
            return None
        if node.kind == SOUND:
            return ("media", node.media, self.loudness(oid)) if node.media else None
        if node.kind == RANSEQ:
            items = node.children
            kind = "random"
        elif node.kind == SWITCH:
            items, kind = self._switch_items(node), "all"
        elif node.kind in (LAYER, ACTOR_MIXER):
            items, kind = node.children, "all"
        else:
            return None
        subtrees = [t for t in (self.tree(c, depth + 1) for c in items) if t]
        if not subtrees:
            return None
        return subtrees[0] if len(subtrees) == 1 else (kind, subtrees)

    def event_tree(self, name):
        event = self.event(name)
        if event is None:
            return None
        subtrees = [t for t in (self.tree(t) for t in self.play_targets(event)) if t]
        if not subtrees:
            return None
        return subtrees[0] if len(subtrees) == 1 else ("all", subtrees)

    def streamed(self, media):
        """Whether a media ID is played from its own .wem file rather than a bank."""
        return any(n.media == media and n.stream for n in self.objects.values() if n.kind == SOUND)


def media_ids(tree):
    if tree is None:
        return []
    if tree[0] == "media":
        return [tree[1]]
    return [m for t in tree[1] for m in media_ids(t)]
