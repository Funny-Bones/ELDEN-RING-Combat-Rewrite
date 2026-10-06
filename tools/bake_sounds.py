"""Bakes the player's sounds into assets/player_sounds.bin and assets/sounds/.

Every clip the sandbox plays has sound events in its TAE: on a given frame,
play sound type T, id N. The game turns that into a Wwise event named
Play_<letter><N, 9 digits> and Wwise picks what to play: one of several
recordings, all layers of a sound, the variant for the floor material. This
resolves each event in the game's own sound banks to that choice tree, and
converts every recording it can reach from Wwise Vorbis to Ogg Vorbis
(losslessly; see wem.py).

Weapon swings are weapon-specific: a weapon's sound offset (EquipParamWeapon)
moves the id by 100 per step, and the swing for that weapon is tried first.

Reads the unpacked sd/ banks; writes only derived files, which are not part
of the repository. Usage: python tools/bake_sounds.py
"""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import paths
import wem
import wwise
from bake_anims import clip_name, wanted_clips
from extract import WEAPONS, Source, gather

ROOT = Path(__file__).parent.parent
OUT = ROOT / "assets" / "player_sounds.bin"
MEDIA = ROOT / "assets" / "sounds"
BANKS = ("sd/enus/cs_main.bnk", "sd/cs_smain.bnk")

EVENT_SOUND = 129
# TAE sound type -> Wwise event letter. 1: character (swings, footsteps on
# the body, voice), 5: effects, 8: floor- and armour-dependent foley (the
# material is a switch inside the event).
LETTERS = {1: "c", 5: "s", 8: "c"}
# EquipParamWeapon.wepSeIdOffset (Paramdex lists it one byte later, at 0x238,
# but its own neighbours sit one byte off there too).
WEAPON_SE_OFFSET = 0x237

TREE_MEDIA, TREE_RANDOM, TREE_ALL = 0, 1, 2


def write_tree(f, tree):
    kind, value = tree[0], tree[1]
    if kind == "media":
        f.write(struct.pack("<BIf", TREE_MEDIA, value, tree[2]))
    else:
        f.write(struct.pack("<BH", TREE_RANDOM if kind == "random" else TREE_ALL, len(value)))
        for child in value:
            write_tree(f, child)


def main():
    banks = []
    for name in BANKS:
        path = paths.er_files() / name
        if not path.exists():
            sys.exit(f"{path} is missing: unpack the sd archive's {name} too (see README)")
        banks.append(wwise.Bank(path.read_bytes()))

    src = Source()
    gathered = gather(src)
    weapons = gathered[0]
    se_offset = {}
    for weapon, (_name, row_id) in zip(weapons, WEAPONS):
        se_offset.setdefault(weapon["file"], struct.unpack_from("<b", src.weapons[row_id], WEAPON_SE_OFFSET)[0])

    trees = []  # unique choice trees
    tree_index = {}  # event name -> index into trees, or None if it plays nothing
    clips = []
    unresolved = set()

    def resolve(name):
        if name not in tree_index:
            tree = next((t for t in (bank.event_tree(name) for bank in banks) if t), None)
            tree_index[name] = None
            if tree is not None:
                tree_index[name] = len(trees)
                trees.append(tree)
        return tree_index[name]

    for file, anim_id in wanted_clips(src, gathered):
        anim = src.anim(file, anim_id)
        if anim is None:
            continue
        offset = se_offset.get(file, 0)
        events = []
        for e in anim.events:
            if e.type != EVENT_SOUND or len(e.params) < 8:
                continue
            kind, sound = e.s32(0), e.s32(1)
            letter = LETTERS.get(kind)
            if letter is None or sound < 0:
                continue
            candidates = [sound + offset * 100, sound] if offset and kind == 1 else [sound]
            index = None
            for sid in candidates:
                index = resolve("Play_%s%09d" % (letter, sid))
                if index is not None:
                    break
            if index is None:
                unresolved.add((kind, sound))
                continue
            events.append((e.start * 30.0, index))
        if events:
            clips.append((clip_name(file, anim_id), sorted(events)))

    # The recordings, as Ogg Vorbis.
    media = sorted({m for t in trees for m in wwise.media_ids(t)})
    MEDIA.mkdir(parents=True, exist_ok=True)
    converted, failed = 0, []
    for mid in media:
        data = next((bank.media[mid] for bank in banks if mid in bank.media), None)
        if data is None:
            loose = paths.er_files() / "sd" / "wem" / str(mid)[:2] / f"{mid}.wem"
            data = loose.read_bytes() if loose.exists() else None
        if data is None:
            failed.append((mid, "not found"))
            continue
        try:
            (MEDIA / f"{mid}.ogg").write_bytes(wem.to_ogg(data))
            converted += 1
        except ValueError as error:
            failed.append((mid, str(error)))
    # Drop trees whose recordings couldn't all be converted.
    have = {mid for mid in media if (MEDIA / f"{mid}.ogg").exists()}

    def prune(tree):
        if tree[0] == "media":
            return tree if tree[1] in have else None
        kept = [t for t in (prune(c) for c in tree[1]) if t]
        if not kept:
            return None
        return kept[0] if len(kept) == 1 else (tree[0], kept)

    trees = [prune(t) for t in trees]

    OUT.parent.mkdir(exist_ok=True)
    with open(OUT, "wb") as f:
        f.write(b"ERSD")
        f.write(struct.pack("<II", 1, len(trees)))
        for tree in trees:
            write_tree(f, tree or ("all", []))
        f.write(struct.pack("<I", len(clips)))
        for name, events in clips:
            raw = name.encode()
            f.write(struct.pack("<B", len(raw)) + raw)
            f.write(struct.pack("<H", len(events)))
            for frame, index in events:
                f.write(struct.pack("<fI", frame, index))
    print(f"wrote {OUT}: {len(clips)} clips with sound, {len(trees)} sounds, {converted} recordings")
    if unresolved:
        print(f"  {len(unresolved)} sound ids play nothing in these banks (left out)")
    for mid, why in failed[:10]:
        print(f"  recording {mid} skipped: {why}")


if __name__ == "__main__":
    main()
