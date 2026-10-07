//! Action definitions and tuning.
//!
//! Everything about individual actions (durations, i-frames, cancel windows,
//! hit windows, root motion, stamina costs, motion values) and the locomotion
//! speeds comes from `extracted.rs`, which `tools/extract.py` generates from
//! the game's own files. Timings are animation frames at 30 fps; the
//! simulation ticks at 60 Hz, so one tick is half a frame.
//!
//! What is still an estimate is marked ESTIMATE below. Those values live in
//! the player script's shared constants or in engine code, which the
//! extractor does not read.

use bevy::math::Vec3;

use super::extracted;
pub use super::extracted::{
    CROUCH_RUN_SPEED, CROUCH_WALK_SPEED, DEFAULT_WEAPON, FIST, MAX_HP, MAX_STAMINA, RUN_BACK_SPEED, RUN_SIDE_SPEED, RUN_SPEED, SHIELD,
    SPRINT_SPEED, TORCH, WALK_SPEED, WEAPONS,
};

pub const ANIM_FPS: f32 = 30.0;
pub const TICK_HZ: f64 = 60.0;
pub const DT: f32 = 1.0 / 60.0;
/// Animation frames advanced per simulation tick.
pub const DF: f32 = ANIM_FPS * DT;

// --- Character (ESTIMATE) --------------------------------------------------

// Max HP and stamina are the starting class's, read from the game's own
// stat curves; see `extracted`.
pub const STAMINA_REGEN: f32 = 45.0;
/// Regen multiplier while the guard is raised.
pub const GUARD_REGEN_MULT: f32 = 0.5;
/// Sprint drain per second. Only applies while an enemy is hostile: out of
/// combat sprinting is free.
pub const SPRINT_DRAIN: f32 = 11.0;
/// Share of incoming stamina damage a raised shield lets through.
pub const GUARD_STAMINA_TAKEN: f32 = 0.55;

// --- Stamina costs, from the game's shared script constants ----------------
// (STAMINA_REDUCE_ROLLING / _BACKSTEP / _JUMP in common_define.hks)

pub const ROLL_COST: f32 = 12.0;
pub const BACKSTEP_COST: f32 = 8.0;
pub const JUMP_COST: f32 = 10.0;

// --- Locomotion (ESTIMATE, speeds themselves are extracted) -----------------

pub const ACCEL: f32 = 26.0;
pub const DECEL: f32 = 32.0;
/// Stick magnitude below which the character walks instead of runs.
pub const WALK_TILT: f32 = 0.55;
pub const STICK_DEADZONE: f32 = 0.1;
/// Turn rates, degrees per second.
pub const TURN_RUN: f32 = 1080.0;
pub const TURN_SPRINT: f32 = 480.0;
pub const TURN_LOCKED: f32 = 720.0;
/// Turn rate inside an action when it neither forbids turning nor sets a rate.
pub const TURN_ACTION_DEFAULT: f32 = 360.0;

/// The dodge button rolls on *release*, and only if it was held for less than
/// this many frames. Held longer, it is a sprint and releasing does nothing.
pub const SPRINT_HOLD_FRAMES: f32 = 10.0;

/// Height the character steps up or down without leaving the ground.
pub const STEP_HEIGHT: f32 = 0.35;

// --- Falling (ESTIMATE; the jump arc itself is extracted) -------------------

pub const GRAVITY: f32 = 18.0;
pub const TERMINAL_VELOCITY: f32 = 40.0;
/// Feet height above ground at which low sweeps pass underneath.
pub const JUMP_CLEARANCE: f32 = 0.35;
/// Frames a jump may stay airborne past the end of its arc before it counts
/// as a fall: it then plays the fall loop and lands like one.
pub const JUMP_BECOMES_FALL: f32 = 3.0;
pub const FALL_HEAVY_LANDING: f32 = 8.0;
pub const FALL_DAMAGE_START: f32 = 16.0;
pub const FALL_DEATH: f32 = 20.0;

// --- Combat (ESTIMATE) -----------------------------------------------------

/// Share of an attack's hit-stop time (which is the game's) that is applied.
/// ESTIMATE: the full time reads as a stall here, so the game presumably
/// does not freeze for all of it.
pub const HIT_STOP_SCALE: f32 = 0.5;
/// Frames after a block during which a heavy attack becomes a guard counter.
pub const GUARD_COUNTER_WINDOW: f32 = 20.0;
/// Frames for the shield to come up before it actually blocks.
pub const GUARD_RAISE_FRAMES: f32 = 4.0;
/// Half-angle of the arc in front of the character a raised shield covers.
pub const GUARD_ARC_DEG: f32 = 80.0;

pub const LOCK_ON_RANGE: f32 = 15.0;
pub const LOCK_BREAK_RANGE: f32 = 22.0;

pub const NEVER: f32 = 9999.0;

/// Equip load tier. Changes which roll you get.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Load {
    Light,
    Medium,
    Heavy,
}

/// A direction relative to facing. For rolls: free rolls are always `Front`
/// (the character turns first); locked on, the four directions are separate
/// animations that keep the character facing its target. For hit reactions:
/// the side the hit came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Front,
    Back,
    Left,
    Right,
}

/// How hard a hit knocks the character about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HurtLevel {
    Small,
    Middle,
    Large,
    /// Knocked off the feet and thrown back.
    Knockdown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JumpKind {
    Stand,
    Walk,
    WalkBack,
    WalkLeft,
    WalkRight,
    Run,
    RunBack,
    RunLeft,
    RunRight,
    Sprint,
}

impl JumpKind {
    /// The walking or running jump that travels toward `side` of facing.
    pub fn toward(side: Dir, walking: bool) -> Self {
        use JumpKind::*;
        match (walking, side) {
            (true, Dir::Front) => Walk,
            (true, Dir::Back) => WalkBack,
            (true, Dir::Left) => WalkLeft,
            (true, Dir::Right) => WalkRight,
            (false, Dir::Front) => Run,
            (false, Dir::Back) => RunBack,
            (false, Dir::Left) => RunLeft,
            (false, Dir::Right) => RunRight,
        }
    }
}

pub struct WeaponInfo {
    pub name: &'static str,
    /// Moveset category: the `aXX` its animations live under.
    pub category: u8,
    /// Base physical attack, unscaled.
    pub attack: f32,
    pub weight: f32,
    /// Animation category of the idle and guard stance: [one-handed, two-handed].
    pub stance: [u8; 2],
}

/// How the armaments are held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grip {
    /// Weapon in the right hand, shield in the left.
    OneHand,
    /// Right-hand weapon in both hands; the shield is put away.
    TwoHandRight,
    /// Left-hand armament (the shield) in both hands; the weapon is put away.
    TwoHandLeft,
}

/// A change of grip or weapon. Each has its own pair of animations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwapKind {
    ToTwoHandRight,
    ToTwoHandLeft,
    ToOneHandFromRight,
    ToOneHandFromLeft,
    NextWeapon,
    /// Changing what the left hand holds.
    NextLeft,
}

impl SwapKind {
    pub fn def(self) -> SwapDef {
        extracted::swap(self)
    }
}

/// Timing of a grip or weapon change, in frames: a `start` animation during
/// which the change takes effect, then an `end` animation.
#[derive(Clone, Copy, Debug)]
pub struct SwapDef {
    pub start: &'static str,
    pub end: &'static str,
    pub start_len: f32,
    pub end_len: f32,
    /// Frame of `start` on which the new grip or weapon is in hand.
    pub apply: f32,
    /// Frame of `end` from which other actions are allowed again.
    pub free_from: f32,
}

impl SwapDef {
    pub fn total(&self) -> f32 {
        self.start_len + self.end_len
    }
}

/// Which set of attack animations applies: a weapon and how it is held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Moveset {
    pub weapon: u8,
    pub two_hand: bool,
}

impl Moveset {
    pub fn attack(self, kind: AttackKind) -> Option<ActionDef> {
        extracted::attack(self.weapon as usize, self.two_hand, kind)
    }

    pub fn has(self, kind: AttackKind) -> bool {
        self.attack(kind).is_some()
    }

    pub fn air(self, heavy: bool) -> Option<AirAttackDef> {
        extracted::air_attack(self.weapon as usize, self.two_hand, heavy)
    }

    /// The jump attack with a weapon in each hand.
    pub fn air_paired(self) -> Option<AirAttackDef> {
        extracted::air_paired(self.weapon as usize)
    }

    pub fn info(self) -> &'static WeaponInfo {
        &WEAPONS[self.weapon as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttackKind {
    Light1,
    Light2,
    Light3,
    Light4,
    Light5,
    Light6,
    RunLight,
    RunHeavy,
    RollAttack,
    CrouchAttack,
    BackstepAttack,
    /// The heavy attack as started: wind-up that either gets released into
    /// `Heavy1` or held through to the charged hit.
    Heavy1Charge,
    Heavy1,
    Heavy2Charge,
    Heavy2,
    GuardCounter,
    /// Landing while a jump attack is still coming down.
    JumpLightLand,
    /// Landing after the jump attack already finished in the air.
    JumpLightLandShort,
    JumpHeavyLand,
    JumpHeavyLandShort,
    /// The left-hand weapon's own chain.
    LeftLight1,
    LeftLight2,
    LeftLight3,
    LeftLight4,
    LeftLight5,
    LeftLight6,
    /// With the same class of weapon in each hand ("power stance"): both
    /// weapons, on the left attack button.
    PairedLight1,
    PairedLight2,
    PairedLight3,
    PairedLight4,
    PairedLight5,
    PairedLight6,
    PairedRun,
    PairedRoll,
    PairedBackstep,
    PairedJumpLand,
    PairedJumpLandShort,
}

impl AttackKind {
    /// The next swing in the light chain, if this is one.
    pub fn next_light(self) -> Option<AttackKind> {
        use AttackKind::*;
        Some(match self {
            Light1 => Light2,
            Light2 => Light3,
            Light3 => Light4,
            Light4 => Light5,
            Light5 => Light6,
            _ => return None,
        })
    }

    /// The next swing of the left hand's chain, if this is one.
    pub fn next_left(self) -> Option<AttackKind> {
        use AttackKind::*;
        Some(match self {
            LeftLight1 => LeftLight2,
            LeftLight2 => LeftLight3,
            LeftLight3 => LeftLight4,
            LeftLight4 => LeftLight5,
            LeftLight5 => LeftLight6,
            _ => return None,
        })
    }

    /// The next swing of the paired chain, if this is one.
    pub fn next_paired(self) -> Option<AttackKind> {
        use AttackKind::*;
        Some(match self {
            PairedLight1 => PairedLight2,
            PairedLight2 => PairedLight3,
            PairedLight3 => PairedLight4,
            PairedLight4 => PairedLight5,
            PairedLight5 => PairedLight6,
            _ => return None,
        })
    }
}

/// A jump attack's swing while still airborne.
#[derive(Clone, Copy, Debug)]
pub struct AirAttackDef {
    /// Active window in frames since the attack started.
    pub from: f32,
    pub to: f32,
    pub stamina: f32,
    pub source: &'static str,
    pub radius: f32,
    /// As `Hit::blade`, for the swing in the air.
    pub blade: &'static [[f32; 6]],
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    /// Active window, `from <= frame < to`.
    pub from: f32,
    pub to: f32,
    /// Motion value: multiplier on attack rating.
    pub mv: f32,
    /// Multiplier on stamina damage dealt to a guarding target.
    pub guard_damage: f32,
    /// Stamina taken when this hit comes out.
    pub stamina: f32,
    /// Seconds attacker and target both freeze for when it lands.
    pub stop: f32,
    /// Thickness of the hit capsule around the blade, metres.
    pub radius: f32,
    /// The blade's two ends on each frame from `from` (rounded down) to `to`
    /// (rounded up), as [x, y, z, x, y, z] in the character's own space.
    pub blade: &'static [[f32; 6]],
}

/// The ends of a blade at `frame`, in the character's own space, from samples
/// that start at the frame `from` rounded down.
pub fn blade_at(blade: &[[f32; 6]], from: f32, frame: f32) -> Option<(Vec3, Vec3)> {
    let last = blade.len().checked_sub(1)?;
    let at = (frame - from.floor()).clamp(0.0, last as f32);
    let (a, b) = (blade[at as usize], blade[(at as usize + 1).min(last)]);
    let t = at.fract();
    let mix = |i: usize| Vec3::new(a[i] + (b[i] - a[i]) * t, a[i + 1] + (b[i + 1] - a[i + 1]) * t, a[i + 2] + (b[i + 2] - a[i + 2]) * t);
    Some((mix(0), mix(3)))
}

#[derive(Clone, Copy, Debug)]
pub struct ActionDef {
    pub name: &'static str,
    /// Game animation this was read from.
    pub source: &'static str,
    /// Frame the animation ends and control returns on its own.
    pub total: f32,
    /// Presses before this frame are ignored rather than queued.
    pub input_from: f32,
    pub input_dodge_from: f32,
    /// Earliest frame each kind of queued input may interrupt.
    pub cancel_light: f32,
    pub cancel_heavy: f32,
    pub cancel_dodge: f32,
    pub cancel_jump: f32,
    pub cancel_guard: f32,
    pub cancel_move: f32,
    /// From when a left-hand attack can take over.
    pub cancel_left: f32,
    /// Invincibility window, `from <= frame < to`.
    pub iframes: (f32, f32),
    /// Low attacks pass underneath while this is airborne.
    pub jump_frames: bool,
    /// Cost of the first hit, for display; each hit carries its own.
    pub stamina: f32,
    /// Every hit of the attack, in order. Most attacks have one.
    pub hits: &'static [Hit],
    /// Window in which releasing the button swaps to the uncharged attack.
    /// Holding past its end commits to the charged one.
    pub charge: Option<(f32, f32)>,
    /// Windows in which the character cannot be turned.
    pub no_turn: &'static [(f32, f32)],
    /// (from, to, degrees per second) turn-rate overrides.
    pub turn: &'static [(f32, f32, f32)],
    /// Root motion, one sample per frame: cumulative [left, up, forward] metres.
    pub motion: &'static [[f32; 3]],
}

impl ActionDef {
    /// Cumulative [left, up, forward] root motion at `frame`.
    pub fn motion_at(&self, frame: f32) -> [f32; 3] {
        let Some(last) = self.motion.len().checked_sub(1) else {
            return [0.0; 3];
        };
        let frame = frame.clamp(0.0, last as f32);
        let i = (frame.floor() as usize).min(last);
        let j = (i + 1).min(last);
        let t = frame - i as f32;
        let (a, b) = (self.motion[i], self.motion[j]);
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    }

    /// The attack's first hit.
    pub fn hit(&self) -> Option<Hit> {
        self.hits.first().copied()
    }

    pub fn can_turn(&self, frame: f32) -> bool {
        !self.no_turn.iter().any(|&(from, to)| frame >= from && frame < to)
    }

    /// Degrees per second the character may turn at on `frame`.
    pub fn turn_rate(&self, frame: f32) -> f32 {
        self.turn
            .iter()
            .find(|&&(from, to, _)| frame >= from && frame < to)
            .map_or(TURN_ACTION_DEFAULT, |&(_, _, rate)| rate)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionId {
    Roll(Load, Dir),
    /// A roll started from a crouch; the character stays crouched.
    CrouchRoll(Load, Dir),
    Backstep,
    SprintStop,
    Jump(JumpKind),
    LandLight,
    LandRun,
    LandSprint,
    /// Landing while circling a locked-on target, walking or running.
    LandStrafeWalk(Dir),
    LandStrafe(Dir),
    LandHeavy,
    /// Landing from walking or rolling off a ledge rather than from a jump.
    LandFall,
    Attack(Moveset, AttackKind),
    GuardHit,
    GuardBreak,
    /// Knocked out of whatever was happening, by a hit from `Dir`.
    Hurt(HurtLevel, Dir),
}

impl ActionId {
    pub fn def(self) -> ActionDef {
        let extracted = match self {
            ActionId::Attack(moveset, kind) => moveset.attack(kind),
            _ => extracted::base(self),
        };
        // Attacks are only ever built for kinds their moveset has, and every
        // other action has an entry, so a miss here is a bug in the generator.
        extracted.unwrap_or_else(|| panic!("no extracted data for {self:?}"))
    }

    /// Stamina taken when the action starts, on top of anything its hit costs.
    /// These are the game's own constants; see `ROLL_COST` and friends.
    pub fn start_cost(self) -> f32 {
        match self {
            ActionId::Roll(..) | ActionId::CrouchRoll(..) => ROLL_COST,
            ActionId::Backstep => BACKSTEP_COST,
            ActionId::Jump(_) => JUMP_COST,
            _ => 0.0,
        }
    }

    /// Ground speed carried out of the action when movement cancels it. ESTIMATE.
    pub fn exit_speed(self) -> f32 {
        match self {
            ActionId::Roll(Load::Heavy, _) => RUN_SPEED * 0.4,
            ActionId::Roll(..) | ActionId::LandRun => RUN_SPEED,
            ActionId::CrouchRoll(..) => CROUCH_RUN_SPEED,
            ActionId::LandSprint => SPRINT_SPEED,
            ActionId::LandStrafeWalk(_) => WALK_SPEED,
            ActionId::LandStrafe(Dir::Front) => RUN_SPEED,
            ActionId::LandStrafe(Dir::Back) => RUN_BACK_SPEED,
            ActionId::LandStrafe(_) => RUN_SIDE_SPEED,
            _ => 0.0,
        }
    }
}
