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

use super::extracted;
pub use super::extracted::{
    CROUCH_RUN_SPEED, CROUCH_WALK_SPEED, DEFAULT_WEAPON, RUN_BACK_SPEED, RUN_SIDE_SPEED, RUN_SPEED, SHIELD, SPRINT_SPEED, WALK_SPEED,
    WEAPONS,
};

pub const ANIM_FPS: f32 = 30.0;
pub const TICK_HZ: f64 = 60.0;
pub const DT: f32 = 1.0 / 60.0;
/// Animation frames advanced per simulation tick.
pub const DF: f32 = ANIM_FPS * DT;

// --- Character (ESTIMATE) --------------------------------------------------

pub const MAX_HP: f32 = 522.0;
pub const MAX_STAMINA: f32 = 96.0;
pub const STAMINA_REGEN: f32 = 45.0;
/// Regen multiplier while the guard is raised.
pub const GUARD_REGEN_MULT: f32 = 0.5;
/// Sprint drain per second. Only applies while an enemy is hostile: out of
/// combat sprinting is free.
pub const SPRINT_DRAIN: f32 = 11.0;
pub const ROLL_COST: f32 = 12.0;
pub const BACKSTEP_COST: f32 = 6.0;
pub const JUMP_COST: f32 = 14.0;
/// Share of incoming stamina damage a raised shield lets through.
pub const GUARD_STAMINA_TAKEN: f32 = 0.55;

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
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JumpKind {
    Stand,
    Walk,
    Run,
    Sprint,
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

/// Blade length per weapon, metres, in `WEAPONS` order. ESTIMATE: used to
/// draw the weapon and to size its hit wedge; the game's models and hit
/// capsules are not extracted.
pub const WEAPON_LENGTH: &[f32] = &[
    0.35, 0.9, 1.3, 1.7, 1.0, 0.95, 0.6, 0.7, 1.9, 2.1, // dagger .. halberd
    1.25, 0.85, 1.35, 1.0, 1.2, 0.8, 1.2, 2.3, 1.7, 2.2, // heavy thrusting sword .. whip
    0.15, 0.3, 1.6, 0.5, // fist, claw, colossal weapon, torch
    0.3, // shield
];

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
}

/// A jump attack's swing while still airborne.
#[derive(Clone, Copy, Debug)]
pub struct AirAttackDef {
    /// Active window in frames since the attack started.
    pub from: f32,
    pub to: f32,
    pub stamina: f32,
    pub source: &'static str,
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
    /// Invincibility window, `from <= frame < to`.
    pub iframes: (f32, f32),
    /// Low attacks pass underneath while this is airborne.
    pub jump_frames: bool,
    /// Spent when the hit window opens, or at the start if there is none.
    pub stamina: f32,
    pub hit: Option<Hit>,
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
    /// ESTIMATE: these are constants in a shared script that is not unpacked.
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
            _ => 0.0,
        }
    }

    /// (range in metres, half-angle in degrees) of the swing. ESTIMATE: the
    /// game sweeps capsules along the blade; this is a wedge in front instead.
    pub fn reach(self) -> (f32, f32) {
        use AttackKind::*;
        let ActionId::Attack(moveset, kind) = self else {
            return (0.0, 0.0);
        };
        let range = 1.4 + WEAPON_LENGTH[moveset.weapon as usize];
        let half_angle = match kind {
            Heavy1 | Heavy1Charge | CrouchAttack | RollAttack | BackstepAttack => 35.0,
            Heavy2 | Heavy2Charge | RunHeavy | JumpHeavyLand | Light5 => 45.0,
            _ => 60.0,
        };
        (range, half_angle)
    }
}
