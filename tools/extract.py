"""Generates src/sim/extracted.rs from unpacked Elden Ring files.

Inputs (unpacked by the user with UXM / WitchyBND, found through the
ER_FILES and ER_GAME_DIR environment variables; see tools/paths.py):
  chr/c0000.anibnd.dcx          animation events (TAE)
  chr/c0000_a00_hi.anibnd.dcx   base-movement animations (root motion)
  chr/c0000_a{2,3,4}x.anibnd.dcx weapon-moveset animations (root motion)
  regulation-bin/*.param        weapons, stamina costs and motion values

Usage: python tools/extract.py
"""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import hkanim
import hkx
import param
import paths
import tae
from erfmt import open_bnd

SRC = paths.er_files()
OUT = Path(__file__).parent.parent / "src" / "sim" / "extracted.rs"
NEVER = 9999.0
PACKS = ("c0000_a00_hi", "c0000_a2x", "c0000_a3x", "c0000_a4x", "c0000_a5x")

# (display name, EquipParamWeapon row). The moveset category, attack rating
# and behaviour variation are read from the row. Rows are the first weapon of
# each class, so the later names are the class rather than a specific weapon.
# The order is shared with WEAPON_LENGTH in data.rs and WEAPON_PARTS in rig.rs,
# and the shield must stay last: it is what the left hand holds.
WEAPONS = [
    ("Dagger", 1000000),
    ("Longsword", 2000000),
    ("Claymore", 3180000),
    ("Greatsword", 4000000),
    ("Rapier", 5000000),
    ("Uchigatana", 9000000),
    ("Club", 11000000),
    ("Battle Axe", 14000000),
    ("Short Spear", 16000000),
    ("Halberd", 18000000),
    ("Heavy Thrusting Sword", 6000000),
    ("Curved Sword", 7000000),
    ("Curved Greatsword", 8010000),
    ("Twinblade", 10000000),
    ("Great Hammer", 12000000),
    ("Flail", 13000000),
    ("Greataxe", 15000000),
    ("Great Spear", 17010000),
    ("Reaper", 19000000),
    ("Whip", 20000000),
    ("Fist", 21000000),
    ("Claw", 22000000),
    ("Colossal Weapon", 23000000),
    ("Torch", 24000000),
    ("Shield", 30000000),
]
DEFAULT_WEAPON = "Longsword"

# One-handed animation ids; the two-handed set is the same ids plus 2000.
ATTACKS = [
    ("Light1", 30000), ("Light2", 30010), ("Light3", 30020),
    ("Light4", 30030), ("Light5", 30040), ("Light6", 30050),
    ("RunLight", 30200), ("RunHeavy", 30210),
    ("RollAttack", 30300), ("CrouchAttack", 30310), ("BackstepAttack", 30400),
    ("Heavy1Charge", 30500), ("Heavy1", 30505), ("Heavy2Charge", 30510), ("Heavy2", 30515),
    ("GuardCounter", 30700),
    ("JumpLightLand", 31070), ("JumpLightLandShort", 31071),
    ("JumpHeavyLand", 31270), ("JumpHeavyLandShort", 31271),
]
AIR_LIGHT, AIR_HEAVY = 31030, 31230
TWO_HAND_OFFSET = 2000

# ChrActionFlag ids (event type 0).
F_NO_TURN, F_DODGING, F_CANCEL_RH, F_CANCEL_MOVE = 7, 8, 4, 11
F_CANCEL_GUARD, F_IN_DODGE, F_CANCEL_DODGE, F_CANCEL_JUMP = 22, 25, 26, 32
F_IN_COMMON, F_CANCEL_R1, F_CANCEL_R2, F_JUMP_FRAMES = 87, 115, 116, 132
SP_CHARGING = 100280
EVENT_BLEND = 16

# Weapon-independent actions: (Rust variant, TAE file, animation id).
BASE = [
    ("Backstep", "a00", 27000),
    ("SprintStop", "a00", 22200),
    ("LandLight", "a00", 202100),
    ("LandRun", "a00", 202127),
    ("LandSprint", "a00", 202125),
    # Landing from a fall rather than a jump: a deep crouch, or from high up
    # a sprawl the character has to get up from.
    ("LandFall", "a00", 202300),
    ("LandHeavy", "a00", 202310),
]
for _load, _base in (("Light", 27100), ("Medium", 27110), ("Heavy", 27120)):
    for _i, _d in enumerate(("Front", "Back", "Left", "Right")):
        BASE.append((f"Roll(Load::{_load}, Dir::{_d})", "a00", _base + _i))
# Rolling while crouched is its own set, and leaves you crouched.
for _load, _base in (("Light", 327100), ("Medium", 327110), ("Heavy", 327120)):
    for _i, _d in enumerate(("Front", "Back", "Left", "Right")):
        BASE.append((f"CrouchRoll(Load::{_load}, Dir::{_d})", "a00", _base + _i))
for _kind, _anim in (("Stand", 202000), ("Walk", 202010), ("Run", 202020), ("Sprint", 202030)):
    BASE.append((f"Jump(JumpKind::{_kind})", "a00", _anim))

# Reactions to being hit: (Rust variant, TAE file, animation id).
#
# The game grades hits by level and picks one of four animations per level;
# which of the four goes with which side is chosen here by how each one
# throws the head (a hit from the front snaps it back, and so on).
REACTIONS = [
    ("Hurt(HurtLevel::Small, Dir::Front)", "a00", 5110),
    ("Hurt(HurtLevel::Small, Dir::Back)", "a00", 5100),
    ("Hurt(HurtLevel::Small, Dir::Left)", "a00", 5120),
    ("Hurt(HurtLevel::Small, Dir::Right)", "a00", 5130),
    ("Hurt(HurtLevel::Middle, Dir::Front)", "a00", 5200),
    ("Hurt(HurtLevel::Middle, Dir::Back)", "a00", 5230),
    ("Hurt(HurtLevel::Middle, Dir::Left)", "a00", 5220),
    ("Hurt(HurtLevel::Middle, Dir::Right)", "a00", 5210),
    ("GuardHit", "a00", 4200),
    ("GuardBreak", "a00", 4270),
]
PACKS_REACTIONS = ("c0000_a00_md", "c0000_a00_lo")  # also hold the swap clips

# Changing grip or weapon: (Rust variant, start animation, end animation).
# Each is a short reach for the weapon, during which the change takes effect,
# followed by a settle. The game plays them on the upper body only. Which end
# follows which start is inferred from how the animations borrow each other.
SWAPS = [
    ("ToTwoHandRight", 29060, 29070),
    ("ToTwoHandLeft", 29080, 29090),
    ("ToOneHandFromRight", 29040, 29050),
    ("ToOneHandFromLeft", 29010, 29020),
    ("NextWeapon", 29000, 29020),
]
EVENT_SET_STYLE, EVENT_SWITCH_WEAPON = 32, 33

# Looping locomotion: speed is root-motion distance over duration.
SPEEDS = [
    ("WALK_SPEED", 20000),
    ("RUN_SPEED", 20100),
    ("RUN_BACK_SPEED", 20101),
    ("RUN_SIDE_SPEED", 20102),
    ("SPRINT_SPEED", 20200),
    ("CROUCH_WALK_SPEED", 320000),
    ("CROUCH_RUN_SPEED", 320100),
]


def frames(t):
    return round(t * 30.0, 2)


def clip_name(file, anim_id):
    return "a%03d_%06d" % (int(file[1:]), anim_id)


class Source:
    def __init__(self):
        bnd = open_bnd(SRC / "chr" / "c0000.anibnd.dcx")
        self.files = bnd
        self.tae_raw = {Path(n).stem: d for _, n, d in bnd if n.endswith(".tae")}
        self.tae = {}
        self.hkx = {}
        for pack in PACKS + PACKS_REACTIONS:
            for _, n, d in open_bnd(SRC / "chr" / f"{pack}.anibnd.dcx"):
                if n.endswith(".hkx"):
                    self.hkx.setdefault(Path(n).stem, d)
        self.behavior = param.rows("BehaviorParam_PC")
        self.atk = param.rows("AtkParam_Pc")
        self.weapons = param.rows("EquipParamWeapon")

    def table(self, file):
        if file not in self.tae:
            self.tae[file] = tae.parse(self.tae_raw[file]) if file in self.tae_raw else {}
        return self.tae[file]

    def anim(self, file, anim_id):
        """The animation whose events apply, following event imports."""
        table = self.table(file)
        anim = table.get(anim_id)
        for _ in range(4):
            if anim is None or anim.import_from is None or anim.import_from not in table:
                break
            anim = table[anim.import_from]
        return anim

    def hkx_name(self, file, anim_id):
        """Name of the HKX this animation plays: its own, or the one it borrows."""
        own = clip_name(file, anim_id)
        if own in self.hkx:
            return own
        entry = self.table(file).get(anim_id)
        if entry is not None and entry.hkx_from is not None:
            borrowed = "a%03d_%06d" % divmod(entry.hkx_from, 1000000)
            if borrowed in self.hkx:
                return borrowed
        return None

    def motion(self, file, anim_id):
        name = self.hkx_name(file, anim_id)
        got = hkx.root_motion(self.hkx[name])
        if not got:
            # Stationary animation with no reference frame: no motion, and
            # its length is that of the skeletal animation itself.
            length = hkanim.Animation(self.hkx[name]).frames_at_30fps()
            return [(0.0, 0.0, 0.0)] * (length + 1)
        _duration, samples = got
        # Havok space here: +X is the character's left, -Z is forward.
        return [(x, y, -z) for x, y, z, _rot in samples]

    def weapon(self, row_id):
        row = self.weapons[row_id]
        (variation,) = struct.unpack_from("<i", row, 0x04)
        (weight,) = struct.unpack_from("<f", row, 0x10)
        (attack,) = struct.unpack_from("<H", row, 0xC8)
        return {
            "category": row[0xE7],
            "variation": variation,
            "attack": attack,
            "weight": weight,
            # Animation category of the idle/guard stance, one- and two-handed.
            "stance": (row[0xF0], row[0xF1]),
        }

    def judge(self, variation, judge_id):
        """(stamina cost, motion value, guard stamina damage multiplier)."""
        # Weapons without their own rows fall back to their class's.
        for var in (variation, variation // 100 * 100):
            row = self.behavior.get(100000000 + var * 1000 + judge_id)
            if row is not None:
                break
        else:
            return None
        (ref_id,) = struct.unpack_from("<i", row, 0x0C)
        (stamina,) = struct.unpack_from("<i", row, 0x14)
        atk = self.atk.get(ref_id)
        if atk is None:
            return None
        (mv,) = struct.unpack_from("<H", atk, 0x3E)
        (stam_dmg,) = struct.unpack_from("<H", atk, 0x46)
        return stamina, mv / 100.0, stam_dmg / 100.0


def windows(anim, flag):
    return sorted((frames(e.start), frames(e.end)) for e in anim.events if e.type == 0 and e.s32(0) == flag)


def first(anim, *flags):
    starts = [w[0] for f in flags for w in windows(anim, f)]
    return min(starts) if starts else NEVER


def fnum(v):
    return "NEVER" if v >= NEVER else f"{v:.1f}"


def hit_events(anim):
    return sorted((frames(e.start), frames(e.end), e.s32(2)) for e in anim.events if e.type == 1)


def blend_frames(src, file, anim_id):
    entry = src.table(file).get(anim_id)
    blends = [e.end * 30.0 for e in entry.events if e.type == EVENT_BLEND] if entry else []
    return blends[0] if blends else 4.0


def action_def(src, name, file, anim_id, variation, reaction=False):
    """Rust `ActionDef { .. }` literal for one animation, or None if it does not exist.

    Reactions carry no input window and their flags mean something narrower
    than on a normal action, so for them: input is always listened for, the
    dodge flag is not treated as invincibility, and a hurt animation frees
    everything at the frame it frees movement."""
    anim = src.anim(file, anim_id)
    if anim is None or src.hkx_name(file, anim_id) is None:
        return None
    motion = src.motion(file, anim_id)
    total = len(motion) - 1
    dodge = windows(anim, F_DODGING)
    # Only the unconditional window counts; later ones carry a state condition.
    iframes = dodge[0] if dodge else (0.0, 0.0)
    common = first(anim, F_IN_COMMON)
    in_dodge = first(anim, F_IN_DODGE)
    cancel = {
        "light": first(anim, F_CANCEL_R1, F_CANCEL_RH),
        "heavy": first(anim, F_CANCEL_R2, F_CANCEL_RH),
        "dodge": first(anim, F_CANCEL_DODGE),
        "jump": first(anim, F_CANCEL_JUMP),
        "guard": first(anim, F_CANCEL_GUARD),
        "move": first(anim, F_CANCEL_MOVE),
    }
    if reaction:
        iframes = (0.0, 0.0)
        common = in_dodge = 0.0
        if name.startswith("Hurt"):
            cancel = {key: cancel["move"] for key in cancel}
    hits = hit_events(anim)
    turns = sorted((frames(e.start), frames(e.end), round(e.f32(0), 1)) for e in anim.events if e.type == 224)
    charging = sorted(
        (frames(e.start), frames(e.end)) for e in anim.events if e.type == 67 and e.s32(0) == SP_CHARGING
    )

    stamina = 0.0
    hit = "None"
    # The first hit that actually does damage: some animations lead with a
    # marker event whose attack row has no motion value.
    for start, end, judge in hits if variation is not None else ():
        got = src.judge(variation, judge)
        if not got:
            print("  ! no behaviour row for", clip_name(file, anim_id), "judge", judge)
            continue
        cost, mv, stam_dmg = got
        if mv <= 0.0:
            continue
        stamina = float(cost)
        hit = "Some(Hit { from: %.1f, to: %.1f, mv: %.2f, guard_damage: %.2f })" % (start, end, mv, stam_dmg)
        break
    charge = "Some((%.1f, %.1f))" % charging[0] if charging else "None"

    fields = [
        'name: "%s"' % name,
        'source: "%s"' % clip_name(file, anim_id),
        "total: %.1f" % total,
        "input_from: %s" % fnum(common),
        "input_dodge_from: %s" % fnum(min(common, in_dodge)),
        "cancel_light: %s" % fnum(cancel["light"]),
        "cancel_heavy: %s" % fnum(cancel["heavy"]),
        "cancel_dodge: %s" % fnum(cancel["dodge"]),
        "cancel_jump: %s" % fnum(cancel["jump"]),
        "cancel_guard: %s" % fnum(cancel["guard"]),
        "cancel_move: %s" % fnum(cancel["move"]),
        "iframes: (%.1f, %.1f)" % iframes,
        "jump_frames: %s" % str(bool(windows(anim, F_JUMP_FRAMES))).lower(),
        "stamina: %.1f" % stamina,
        "hit: %s" % hit,
        "charge: %s" % charge,
        "no_turn: &[" + ", ".join("(%.1f, %.1f)" % w for w in windows(anim, F_NO_TURN)) + "]",
        "turn: &[" + ", ".join("(%.1f, %.1f, %.1f)" % t for t in turns) + "]",
        "motion: &[" + ", ".join("[%.3f, %.3f, %.3f]" % m for m in motion) + "]",
    ]
    return "ActionDef { " + ", ".join(fields) + " }"


def swap_def(src, start, end):
    """Rust `SwapDef { .. }` literal for one grip or weapon change."""
    start_anim, end_anim = src.anim("a00", start), src.anim("a00", end)
    start_len = hkanim.Animation(src.hkx[src.hkx_name("a00", start)]).frames_at_30fps()
    end_len = hkanim.Animation(src.hkx[src.hkx_name("a00", end)]).frames_at_30fps()
    applies = [frames(e.start) for e in start_anim.events if e.type in (EVENT_SET_STYLE, EVENT_SWITCH_WEAPON)]
    return 'SwapDef { start: "%s", end: "%s", start_len: %.1f, end_len: %.1f, apply: %.1f, free_from: %.1f }' % (
        clip_name("a00", start),
        clip_name("a00", end),
        start_len,
        end_len,
        applies[0],
        first(end_anim, F_CANCEL_RH),
    )


def gather(src):
    """Everything the generator and the animation baker need, in one pass."""
    weapons = []
    for name, row_id in WEAPONS:
        info = src.weapon(row_id)
        info["name"] = name
        info["file"] = "a%d" % info["category"]
        weapons.append(info)

    attacks = []  # (weapon index, two_hand, kind, literal, file, anim id)
    air = []  # (weapon index, two_hand, heavy, from, to, cost, file, anim id)
    for index, weapon in enumerate(weapons):
        for two_hand in (False, True):
            offset = TWO_HAND_OFFSET if two_hand else 0
            for kind, base_id in ATTACKS:
                anim_id = base_id + offset
                literal = action_def(src, kind, weapon["file"], anim_id, weapon["variation"])
                if not literal:
                    continue
                # A few paired-weapon attacks carry no hit event of their own.
                # A swing that can never connect is worse than not having it:
                # leaving it out makes the moveset fall back to another attack.
                if "hit: None" in literal and not kind.endswith("Short"):
                    print("  skipped (no hit window):", weapon["name"], "2H" if two_hand else "1H", kind)
                    continue
                attacks.append((index, two_hand, kind, literal, weapon["file"], anim_id))
            for heavy, base_id in ((False, AIR_LIGHT), (True, AIR_HEAVY)):
                anim_id = base_id + offset
                anim = src.anim(weapon["file"], anim_id)
                if anim is None or src.hkx_name(weapon["file"], anim_id) is None or not hit_events(anim):
                    continue
                start, end, judge = hit_events(anim)[0]
                got = src.judge(weapon["variation"], judge)
                cost = got[0] if got else 0
                air.append((index, two_hand, heavy, start, end, cost, weapon["file"], anim_id))
    return weapons, attacks, air


def main():
    src = Source()
    weapons, attacks, air = gather(src)
    out = [
        "//! GENERATED by tools/extract.py from the game's own files. Do not edit.",
        "//!",
        "//! Timings are animation frames at 30 fps, read from the player's TAE event",
        "//! files. Motion is root motion sampled once per frame from the HKX",
        "//! animations, as cumulative [left, up, forward] metres. Weapons, stamina",
        "//! costs and motion values come from EquipParamWeapon, BehaviorParam_PC and",
        "//! AtkParam_Pc.",
        "",
        "use super::data::{",
        "    ActionDef, ActionId, AirAttackDef, AttackKind, Dir, Hit, HurtLevel, JumpKind, Load, SwapDef, SwapKind,",
        "    WeaponInfo, NEVER,",
        "};",
        "",
    ]
    for const, anim_id in SPEEDS:
        motion = src.motion("a00", anim_id)
        end = motion[-1]
        dist = (end[0] ** 2 + end[2] ** 2) ** 0.5
        out.append("pub const %s: f32 = %.3f;" % (const, dist / ((len(motion) - 1) / 30.0)))

    out += ["", "pub const WEAPONS: &[WeaponInfo] = &["]
    for weapon in weapons:
        out.append(
            '    WeaponInfo { name: "%s", category: %d, attack: %.1f, weight: %.1f, stance: [%d, %d] },'
            % (weapon["name"], weapon["category"], weapon["attack"], weapon["weight"], *weapon["stance"])
        )
        print(weapon["name"], "category", weapon["category"], "attack", weapon["attack"])
    names = [w["name"] for w in weapons]
    out += [
        "];",
        "pub const DEFAULT_WEAPON: usize = %d;" % names.index(DEFAULT_WEAPON),
        "/// What the left hand holds.",
        "pub const SHIELD: usize = %d;" % (len(weapons) - 1),
        "",
        "#[rustfmt::skip]",
        "pub fn base(id: ActionId) -> Option<ActionDef> {",
        "    use ActionId::*;",
        "    Some(match id {",
    ]
    for variant, file, anim_id in BASE:
        label = variant.replace("Load::", "").replace("Dir::", "").replace("JumpKind::", "")
        out.append("        %s => %s," % (variant, action_def(src, label, file, anim_id, None)))
    for variant, file, anim_id in REACTIONS:
        label = variant.replace("HurtLevel::", "").replace("Dir::", "")
        out.append("        %s => %s," % (variant, action_def(src, label, file, anim_id, None, reaction=True)))
    out += ["        _ => return None,", "    })", "}", ""]

    out += [
        "#[rustfmt::skip]",
        "pub fn attack(weapon: usize, two_hand: bool, kind: AttackKind) -> Option<ActionDef> {",
        "    use AttackKind::*;",
        "    Some(match (weapon, two_hand, kind) {",
    ]
    for index, two_hand, kind, literal, _file, _anim_id in attacks:
        out.append("        (%d, %s, %s) => %s," % (index, str(two_hand).lower(), kind, literal))
    out += ["        _ => return None,", "    })", "}", ""]

    out += [
        "/// Jump attacks while still in the air.",
        "#[rustfmt::skip]",
        "pub fn air_attack(weapon: usize, two_hand: bool, heavy: bool) -> Option<AirAttackDef> {",
        "    Some(match (weapon, two_hand, heavy) {",
    ]
    for index, two_hand, heavy, start, end, cost, file, anim_id in air:
        out.append(
            '        (%d, %s, %s) => AirAttackDef { from: %.1f, to: %.1f, stamina: %.1f, source: "%s" },'
            % (index, str(two_hand).lower(), str(heavy).lower(), start, end, cost, clip_name(file, anim_id))
        )
    out += ["        _ => return None,", "    })", "}", ""]

    out += [
        "/// Grip and weapon changes.",
        "#[rustfmt::skip]",
        "pub fn swap(kind: SwapKind) -> SwapDef {",
        "    match kind {",
    ]
    for variant, start, end in SWAPS:
        out.append("        SwapKind::%s => %s," % (variant, swap_def(src, start, end)))
    out += ["    }", "}", ""]

    OUT.write_text("\n".join(out), encoding="utf-8", newline="\n")
    print("wrote", OUT, "-", len(attacks), "attacks,", len(air), "air attacks")


if __name__ == "__main__":
    main()
