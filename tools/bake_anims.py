"""Bakes the player's real animations into assets/player_anims.bin.

Each clip is decoded from the game's spline-compressed HKX, posed on the
c0000 skeleton, and stored as model-space joint positions (plus full axes for
the few joints whose orientation matters) at 30 fps. The game's model space
is left-handed (+X left, +Y up, -Z forward); everything is written mirrored
into the sandbox's right-handed space (+X left, +Y up, +Z forward).

Usage: python tools/bake_anims.py
"""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import hkanim
import paths
from erfmt import open_bnd
from extract import BASE, REACTIONS, SWAPS, Source, blend_frames, gather
from skel import fk, qrot

SRC = paths.er_files() / "chr"
OUT = Path(__file__).parent.parent / "assets" / "player_anims.bin"
PACKS = ("c0000_a00_hi", "c0000_a00_lo", "c0000_a00_md", "c0000_a0x", "c0000_a1x", "c0000_a2x", "c0000_a3x", "c0000_a4x", "c0000_a5x")

JOINTS = [
    "Pelvis", "Spine", "Spine1", "Spine2", "Neck", "Head",
    "L_Clavicle", "L_UpperArm", "L_Forearm", "L_Hand",
    "R_Clavicle", "R_UpperArm", "R_Forearm", "R_Hand",
    "L_Thigh", "L_Calf", "L_Foot", "L_Toe0",
    "R_Thigh", "R_Calf", "R_Foot", "R_Toe0",
    "R_Weapon", "L_Weapon",
    # Each hand: the thumb's three bones, the index knuckle (for which way
    # the palm faces) and the middle finger's three, which bend the mitten.
    *[side + bone for side in ("L_", "R_") for bone in ("Finger0", "Finger01", "Finger02", "Finger1", "Finger2", "Finger21", "Finger22")],
]
# Joints that also store their X, Y and Z axes.
ORIENTED = ["Head", "R_Weapon", "L_Weapon"]
# Clips beyond the ones backing an extracted action: (TAE file, animation id).
EXTRA = [
    ("a00", 20000), ("a00", 20001), ("a00", 20002), ("a00", 20003),  # walk F/B/L/R
    ("a00", 20100), ("a00", 20101), ("a00", 20102), ("a00", 20103),  # run
    ("a00", 20200),    # sprint
    ("a00", 300000),   # crouch idle
    ("a00", 320000), ("a00", 320001), ("a00", 320002), ("a00", 320003),  # crouch walk
    ("a00", 320100), ("a00", 320101), ("a00", 320102), ("a00", 320103),  # crouch run
    ("a00", 22100), ("a00", 22101), ("a00", 22102), ("a00", 22103),  # run stop
    ("a00", 322100),   # crouch run stop
    ("a00", 390000), ("a00", 390001),  # crouching down, standing back up
    ("a00", 202040),   # airborne loop, for jumps and falls alike
    ("a00", 4000),     # start of a fall off a ledge
    ("a00", 17002),    # death
]

STANCE_IDLE, STANCE_GUARD = 0, 100
# ...and the run's stop in each direction.
STANCE_LOCOMOTION = (20000, 20001, 20002, 20003, 20100, 20101, 20102, 20103, 20200, 22100, 22101, 22102, 22103)


def clip_name(file, anim_id):
    return "a%03d_%06d" % (int(file[1:]), anim_id)


def mirror(v):
    return (v[0], v[1], -v[2])


def wanted_clips(src, gathered=None):
    """(TAE file, animation id) of every clip the sandbox plays, in a stable order.
    `gathered` is extract.gather(src), if the caller already has it."""
    weapons, attacks, air = gathered or gather(src)
    wanted = [(file, anim_id) for _variant, file, anim_id in BASE + REACTIONS] + EXTRA
    wanted += [("a00", anim_id) for _variant, start, end in SWAPS for anim_id in (start, end)]
    # Every stance a weapon can be held in: idle and guard, plus the full
    # locomotion set that two-handed stances carry.
    for weapon in weapons:
        one, two = weapon["stance"]
        wanted += [("a%02d" % one, STANCE_IDLE), ("a%02d" % one, STANCE_GUARD)]
        wanted += [("a%02d" % two, anim_id) for anim_id in (STANCE_IDLE, STANCE_GUARD, *STANCE_LOCOMOTION)]
    wanted = [(file, anim_id) for file, anim_id in wanted if src.hkx_name(file, anim_id)]
    wanted += [(file, anim_id) for *_rest, file, anim_id in attacks]
    wanted += [(file, anim_id) for *_rest, file, anim_id in air]
    return list(dict.fromkeys(wanted))


def main():
    src = Source()
    skeleton = next(d for _, n, d in src.files if n.endswith("Skeleton.hkx"))
    names, parents, rest = hkanim.skeleton(skeleton)
    hkx = dict(src.hkx)
    for pack in PACKS:
        for _, n, d in open_bnd(SRC / f"{pack}.anibnd.dcx"):
            if n.endswith(".hkx"):
                hkx.setdefault(Path(n).stem, d)
    src.hkx = hkx

    joint_index = [names.index(j) for j in JOINTS]
    oriented_index = [names.index(j) for j in ORIENTED]

    clips = []
    for file, anim_id in wanted_clips(src):
        name = clip_name(file, anim_id)
        # An animation may borrow another one's HKX.
        source = src.hkx_name(file, anim_id)
        anim = hkanim.Animation(hkx[source])
        blend = blend_frames(src, file, anim_id)

        frames = []
        # Always bake at 30 fps, by time: some clips are authored at 60.
        for frame in range(anim.frames_at_30fps() + 1):
            local = list(rest)
            for bone, transform in zip(anim.bones, anim.sample_seconds(frame / 30.0)):
                local[bone] = transform
            model = fk(parents, local)
            row = []
            for i in joint_index:
                row += mirror(model[i][0])
            for i in oriented_index:
                rotation = model[i][1]
                for axis in ((1, 0, 0), (0, 1, 0), (0, 0, 1)):
                    row += mirror(qrot(rotation, axis))
            frames.append(row)
        clips.append((name, blend, frames))

    OUT.parent.mkdir(exist_ok=True)
    with open(OUT, "wb") as f:
        f.write(b"ERAN")
        f.write(struct.pack("<III", len(JOINTS), len(ORIENTED), len(clips)))
        for group in (JOINTS, ORIENTED):
            for joint in group:
                raw = joint.encode()
                f.write(struct.pack("<B", len(raw)) + raw)
        for name, blend, frames in clips:
            raw = name.encode()
            f.write(struct.pack("<B", len(raw)) + raw)
            f.write(struct.pack("<fI", blend, len(frames)))
            for row in frames:
                f.write(struct.pack("<%df" % len(row), *row))
    print("wrote", OUT, OUT.stat().st_size, "bytes,", len(clips), "clips")


if __name__ == "__main__":
    main()
