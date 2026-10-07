//! The player state machine. Pure data in, pure data out: no engine types
//! beyond vector math, so it can be stepped headlessly in tests.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use bevy::math::{Vec2, Vec3};

use super::data::*;
use super::level::Level;
use super::{angle_diff, approach, dir_of, turn_toward, yaw_of};

#[derive(Clone, Copy, Default, Debug)]
pub struct Button {
    pub held: bool,
    /// Went down since the last tick.
    pub pressed: bool,
    /// Went up since the last tick.
    pub released: bool,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Input {
    /// Movement stick: x right, y forward, relative to the camera.
    pub mv: Vec2,
    pub cam_yaw: f32,
    pub dodge: Button,
    pub jump: Button,
    pub light: Button,
    pub heavy: Button,
    pub guard: Button,
    pub crouch: bool,
    pub lock: bool,
    pub walk: bool,
    /// Toggle two-handing the right-hand weapon / the left-hand armament.
    pub two_hand_right: bool,
    pub two_hand_left: bool,
    /// Swap to the next right-hand weapon.
    pub next_weapon: bool,
    /// Swap what the left hand holds.
    pub next_left: bool,
}

impl Input {
    pub fn tilt(&self) -> f32 {
        self.mv.length().min(1.0)
    }

    /// World-space direction the stick is asking for, if it is out of the deadzone.
    pub fn wish(&self) -> Option<Vec3> {
        if self.mv.length() < STICK_DEADZONE {
            return None;
        }
        let forward = dir_of(self.cam_yaw);
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        Some((forward * self.mv.y + right * self.mv.x).normalize())
    }
}

/// An action request waiting for the current animation to allow it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Req {
    Light,
    Heavy,
    /// The left-hand attack, when the left hand holds something to attack with.
    Left,
    Dodge,
    Jump,
}

/// What the left-hand button does with the current armaments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeftButton {
    Guard,
    Attack,
}

#[derive(Clone, Copy, Debug)]
pub struct Act {
    pub id: ActionId,
    pub f: f32,
    /// Bit per hit of the attack: which have already connected...
    landed: u32,
    /// ...and which have had their stamina cost taken.
    paid: u32,
}

/// A grip or weapon change in progress. It plays on the upper body, so it
/// runs alongside movement instead of being an action of its own.
#[derive(Clone, Copy, Debug)]
pub struct Swap {
    pub kind: SwapKind,
    pub f: f32,
    /// What will be in hand once the change takes effect.
    grip: Grip,
    weapon: usize,
    left: usize,
    applied: bool,
}

/// A jump attack in progress. It runs on its own clock over the jump or fall.
#[derive(Clone, Copy, Debug)]
pub struct AirAttack {
    pub heavy: bool,
    /// A weapon in each hand, both coming down.
    pub paired: bool,
    pub moveset: Moveset,
    pub def: AirAttackDef,
    /// Frames since the attack was started.
    pub f: f32,
    pub hit_done: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Air {
    pub vel: Vec3,
    /// Came from a jump rather than walking off a ledge.
    pub jumped: bool,
    /// Frames spent in the air since leaving the jump arc or the ledge.
    pub f: f32,
}

impl Air {
    /// Dropping, as opposed to still hanging in a jump: either it never was a
    /// jump, or the jump has gone on well past its own arc (off a ledge, say).
    pub fn falling(&self) -> bool {
        !self.jumped || self.f >= JUMP_BECOMES_FALL
    }
}

#[derive(Clone, Copy, Debug)]
pub enum State {
    Ground,
    Act(Act),
    Air(Air),
    Dead { t: f32 },
}

pub struct Incoming {
    pub damage: f32,
    pub stamina: f32,
    pub from: Vec3,
    /// A low sweep: passes under a character who is in the air.
    pub low: bool,
    pub level: HurtLevel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitResult {
    Ignored,
    Dodged,
    Jumped,
    Blocked,
    GuardBroken,
    Hit,
    Killed,
}

/// The swing that is live this tick.
#[derive(Clone, Copy, Debug)]
pub struct ActiveHit {
    /// Base attack of the weapon doing the hitting.
    pub attack: f32,
    pub mv: f32,
    pub guard_damage: f32,
    /// Seconds both sides freeze for if it lands.
    pub stop: f32,
    /// Thickness of the hit capsule around the blade.
    pub radius: f32,
    /// Where the blade passed during this tick: its two ends, in the world,
    /// at a few moments from the last tick to this one.
    pub sweep: [(Vec3, Vec3); SWEEP_STEPS],
}

/// Positions of the blade checked per tick. A fast swing moves the tip a
/// third of a metre in a tick, so one check would let it pass through things.
pub const SWEEP_STEPS: usize = 5;

const RESPAWN_FRAMES: f32 = 90.0;

#[derive(Clone, Debug)]
pub struct Player {
    pub pos: Vec3,
    pub yaw: f32,
    pub hp: f32,
    pub stamina: f32,
    pub state: State,
    pub speed: f32,
    pub move_dir: Vec3,
    pub sprinting: bool,
    pub crouching: bool,
    pub guarding: bool,
    pub load: Load,
    /// Index into `WEAPONS` of the right-hand weapon.
    pub weapon: usize,
    /// And of what the left hand holds: the shield, a torch, another weapon,
    /// or nothing (`FIST`).
    pub left: usize,
    pub grip: Grip,
    pub in_combat: bool,
    pub buffer: Option<Req>,
    pub air_attack: Option<AirAttack>,
    pub swap: Option<Swap>,
    /// Seconds left frozen after landing a hit: the weapon biting.
    pub hit_stop: f32,
    pub spawn: Vec3,
    guard_t: f32,
    guard_counter: f32,
    dodge_hold: f32,
    dodge_armed: bool,
    /// Sprint ran the bar dry; the button must be released before sprinting again.
    sprint_spent: bool,
    /// Ground height the current jump took off from.
    jump_base: f32,
    /// Highest point reached since leaving the ground, for fall damage.
    peak: f32,
}

impl Player {
    pub fn new(spawn: Vec3) -> Self {
        Self {
            pos: spawn,
            yaw: 0.0,
            hp: MAX_HP,
            stamina: MAX_STAMINA,
            state: State::Ground,
            speed: 0.0,
            move_dir: Vec3::Z,
            sprinting: false,
            crouching: false,
            guarding: false,
            load: Load::Medium,
            weapon: DEFAULT_WEAPON,
            left: SHIELD,
            grip: Grip::OneHand,
            in_combat: false,
            buffer: None,
            air_attack: None,
            swap: None,
            hit_stop: 0.0,
            spawn,
            guard_t: 0.0,
            guard_counter: 0.0,
            dodge_hold: 0.0,
            dodge_armed: false,
            sprint_spent: false,
            jump_base: spawn.y,
            peak: spawn.y,
        }
    }

    /// The attack animations in effect for the current weapon and grip.
    pub fn moveset(&self) -> Moveset {
        match self.grip {
            Grip::OneHand => Moveset { weapon: self.weapon as u8, two_hand: false },
            Grip::TwoHandRight => Moveset { weapon: self.weapon as u8, two_hand: true },
            Grip::TwoHandLeft => Moveset { weapon: self.left as u8, two_hand: true },
        }
    }

    /// The same class of weapon in each hand, which fights as a pair.
    pub fn paired(&self) -> bool {
        self.grip == Grip::OneHand && self.left == self.weapon && self.moveset().has(AttackKind::PairedLight1)
    }

    /// A shield guards, and so does any weapon held in both hands. Anything
    /// else in the left hand attacks.
    pub fn left_button(&self) -> LeftButton {
        if self.grip == Grip::OneHand && self.left != SHIELD {
            LeftButton::Attack
        } else {
            LeftButton::Guard
        }
    }

    /// The first attack this pair of armaments has for the left button: of
    /// `paired` if they fight as a pair, else of `single` from the left weapon.
    fn pick_left(&self, paired: &[AttackKind], single: &[AttackKind]) -> Option<ActionId> {
        if self.paired() {
            return self.pick(paired);
        }
        let moveset = Moveset { weapon: self.left as u8, two_hand: false };
        single.iter().copied().find(|&kind| moveset.has(kind)).map(|kind| ActionId::Attack(moveset, kind))
    }

    /// The first of `kinds` this moveset actually has.
    fn pick(&self, kinds: &[AttackKind]) -> Option<ActionId> {
        let moveset = self.moveset();
        kinds.iter().copied().find(|&kind| moveset.has(kind)).map(|kind| ActionId::Attack(moveset, kind))
    }

    pub fn facing(&self) -> Vec3 {
        dir_of(self.yaw)
    }

    fn left(&self) -> Vec3 {
        let f = self.facing();
        Vec3::new(f.z, 0.0, -f.x)
    }

    pub fn is_dead(&self) -> bool {
        matches!(self.state, State::Dead { .. })
    }

    pub fn airborne(&self) -> bool {
        match self.state {
            State::Air(_) => true,
            State::Act(a) => matches!(a.id, ActionId::Jump(_)) && a.id.def().motion_at(a.f)[1] > 0.0,
            _ => false,
        }
    }

    pub fn invincible(&self) -> bool {
        match self.state {
            State::Act(a) => {
                let (from, to) = a.id.def().iframes;
                a.f >= from && a.f < to
            }
            State::Dead { .. } => true,
            _ => false,
        }
    }

    pub fn guard_up(&self) -> bool {
        self.guarding && self.guard_t >= GUARD_RAISE_FRAMES
    }

    pub fn guard_counter_ready(&self) -> bool {
        self.guard_counter > 0.0
    }

    /// The hit volume that is live this tick, if the current attack has not
    /// already connected.
    pub fn active_hit(&self) -> Option<ActiveHit> {
        if let Some(attack) = self.air_attack {
            let def = attack.def;
            let live = !attack.hit_done && attack.f >= def.from && attack.f < def.to;
            // The airborne swing deals what its landing follow-through does.
            let kind = if attack.paired {
                AttackKind::PairedJumpLand
            } else if attack.heavy {
                AttackKind::JumpHeavyLand
            } else {
                AttackKind::JumpLightLand
            };
            let hit = attack.moveset.attack(kind).and_then(|landing| landing.hit())?;
            let sweep = self.sweep(def.blade, def.from, attack.f)?;
            return live.then_some(ActiveHit {
                attack: attack.moveset.info().attack,
                mv: hit.mv,
                guard_damage: hit.guard_damage,
                stop: hit.stop,
                radius: def.radius,
                sweep,
            });
        }
        let State::Act(a) = self.state else {
            return None;
        };
        let ActionId::Attack(moveset, _) = a.id else {
            return None;
        };
        let (_, hit) = Self::live_hit(&a)?;
        Some(ActiveHit {
            attack: moveset.info().attack,
            mv: hit.mv,
            guard_damage: hit.guard_damage,
            stop: hit.stop,
            radius: hit.radius,
            sweep: self.sweep(hit.blade, hit.from, a.f)?,
        })
    }

    /// The blade's path over the tick that ended at `frame`, in the world.
    fn sweep(&self, blade: &[[f32; 6]], from: f32, frame: f32) -> Option<[(Vec3, Vec3); SWEEP_STEPS]> {
        let (forward, left) = (self.facing(), self.left());
        let place = |v: Vec3| self.pos + left * v.x + Vec3::Y * v.y + forward * v.z;
        let start = (frame - DF).max(from);
        let mut sweep = [(Vec3::ZERO, Vec3::ZERO); SWEEP_STEPS];
        for (i, slot) in sweep.iter_mut().enumerate() {
            let at = start + (frame - start) * i as f32 / (SWEEP_STEPS - 1) as f32;
            let (a, b) = blade_at(blade, from, at)?;
            *slot = (place(a), place(b));
        }
        Some(sweep)
    }

    /// The hit of the action whose window is open and which has not connected yet.
    fn live_hit(a: &Act) -> Option<(usize, Hit)> {
        a.id.def()
            .hits
            .iter()
            .copied()
            .enumerate()
            .find(|&(i, hit)| a.f >= hit.from && a.f < hit.to && a.landed & (1 << i) == 0)
    }

    pub fn mark_hit(&mut self) {
        if let Some(attack) = &mut self.air_attack {
            attack.hit_done = true;
        } else if let State::Act(a) = &mut self.state {
            if let Some((i, _)) = Self::live_hit(a) {
                a.landed |= 1 << i;
            }
        }
    }

    pub fn step(&mut self, inp: &Input, level: &Level, target: Option<Vec3>, in_combat: bool) {
        self.in_combat = in_combat;
        if let State::Dead { t } = &mut self.state {
            *t += DF;
            if *t >= RESPAWN_FRAMES {
                let load = self.load;
                *self = Player::new(self.spawn);
                self.load = load;
            }
            return;
        }

        self.read_buttons(inp);
        // Frozen on a hit: inputs are still heard, nothing else moves.
        if self.hit_stop > 0.0 {
            self.hit_stop -= DT;
            return;
        }
        self.guard_counter = (self.guard_counter - DF).max(0.0);
        if let Some(attack) = &mut self.air_attack {
            attack.f += DF;
        }
        if let Some(mut swap) = self.swap {
            let def = swap.kind.def();
            swap.f += DF;
            if !swap.applied && swap.f >= def.apply {
                self.grip = swap.grip;
                self.weapon = swap.weapon;
                self.left = swap.left;
                swap.applied = true;
            }
            self.swap = (swap.f < def.total()).then_some(swap);
        }

        let regen = match self.state {
            State::Ground => self.ground(inp, level, target),
            State::Act(a) => self.act(a, inp, level, target),
            State::Air(a) => {
                self.air(a, inp, level, target.is_some());
                false
            }
            State::Dead { .. } => false,
        };

        if regen {
            let mult = if self.guarding { GUARD_REGEN_MULT } else { 1.0 };
            self.stamina = (self.stamina + STAMINA_REGEN * mult * DT).min(MAX_STAMINA);
        }
    }

    fn read_buttons(&mut self, inp: &Input) {
        // Each animation says from which frame it starts listening for the
        // next input. Anything pressed earlier is simply lost.
        let (listening, listening_dodge) = match self.state {
            State::Act(a) if !matches!(a.id, ActionId::Jump(_)) => {
                let def = a.id.def();
                (a.f >= def.input_from, a.f >= def.input_dodge_from)
            }
            _ => (true, true),
        };

        if inp.dodge.pressed {
            self.dodge_hold = 0.0;
            self.dodge_armed = true;
        }
        if inp.dodge.held {
            self.dodge_hold += DF;
        }
        if inp.dodge.released {
            // The roll comes out on release, and only for a short press.
            if self.dodge_armed && self.dodge_hold < SPRINT_HOLD_FRAMES && listening_dodge {
                self.buffer = Some(Req::Dodge);
            }
            self.dodge_armed = false;
            self.dodge_hold = 0.0;
            self.sprint_spent = false;
        }
        // One slot, newest input wins.
        if listening {
            if inp.jump.pressed {
                self.buffer = Some(Req::Jump);
            }
            if inp.heavy.pressed {
                self.buffer = Some(Req::Heavy);
            }
            if inp.guard.pressed && self.left_button() == LeftButton::Attack {
                self.buffer = Some(Req::Left);
            }
            if inp.light.pressed {
                self.buffer = Some(Req::Light);
            }
        }
    }

    /// Mid grip or weapon change, before the point other actions are allowed.
    pub fn swap_busy(&self) -> bool {
        self.swap.is_some_and(|swap| {
            let def = swap.kind.def();
            swap.f < def.start_len + def.free_from
        })
    }

    fn sprint_held(&self, inp: &Input) -> bool {
        inp.dodge.held && self.dodge_hold >= SPRINT_HOLD_FRAMES
    }

    fn ground(&mut self, inp: &Input, level: &Level, target: Option<Vec3>) -> bool {
        let wish = inp.wish();

        // Hands that are busy changing grip cannot start anything; the press waits.
        if self.swap_busy() {
        } else if let Some(req) = self.buffer.take() {
            if self.stamina > 0.0 {
                use AttackKind::*;
                let attack = match req {
                    Req::Dodge => {
                        self.start_dodge(wish, target);
                        return false;
                    }
                    Req::Jump => {
                        self.start_jump(inp, target);
                        return false;
                    }
                    Req::Left if self.sprinting => self.pick_left(&[PairedRun, PairedLight1], &[LeftLight1]),
                    Req::Left => self.pick_left(&[PairedLight1], &[LeftLight1]),
                    Req::Light if self.sprinting => self.pick(&[RunLight, Light1]),
                    Req::Light if self.crouching => self.pick(&[CrouchAttack, RollAttack, Light1]),
                    Req::Light => self.pick(&[Light1]),
                    Req::Heavy if self.sprinting => self.pick(&[RunHeavy, Heavy1Charge]),
                    Req::Heavy if self.guard_counter_ready() => self.pick(&[GuardCounter, Heavy1Charge]),
                    Req::Heavy => self.pick(&[Heavy1Charge, Heavy1]),
                };
                if let Some(attack) = attack {
                    self.start(attack);
                    return false;
                }
            }
        }

        // Changing grip or weapon only starts from a neutral stance, one at a time.
        if self.swap.is_none() {
            let back = match self.grip {
                Grip::TwoHandLeft => SwapKind::ToOneHandFromLeft,
                _ => SwapKind::ToOneHandFromRight,
            };
            let change = if inp.two_hand_right {
                Some(match self.grip {
                    Grip::TwoHandRight => (back, Grip::OneHand, self.weapon, self.left),
                    _ => (SwapKind::ToTwoHandRight, Grip::TwoHandRight, self.weapon, self.left),
                })
            } else if inp.two_hand_left {
                Some(match self.grip {
                    Grip::TwoHandLeft => (back, Grip::OneHand, self.weapon, self.left),
                    _ => (SwapKind::ToTwoHandLeft, Grip::TwoHandLeft, self.weapon, self.left),
                })
            } else if inp.next_weapon {
                // The shield is the last entry and is never a right-hand weapon.
                Some((SwapKind::NextWeapon, self.grip, (self.weapon + 1) % SHIELD, self.left))
            } else if inp.next_left && self.grip == Grip::OneHand {
                Some((SwapKind::NextLeft, self.grip, self.weapon, next_left_hand(self.left)))
            } else {
                None
            };
            if let Some((kind, grip, weapon, left)) = change {
                self.swap = Some(Swap { kind, f: 0.0, grip, weapon, left, applied: false });
            }
        }

        if inp.crouch {
            self.crouching = !self.crouching;
        }

        let was_sprinting = self.sprinting;
        self.sprinting =
            self.sprint_held(inp) && wish.is_some() && !self.sprint_spent && self.stamina > 0.0;
        if self.sprinting {
            self.crouching = false;
            if self.in_combat {
                self.stamina -= SPRINT_DRAIN * DT;
                if self.stamina <= 0.0 {
                    self.stamina = 0.0;
                    self.sprinting = false;
                    self.sprint_spent = true;
                }
            }
        }
        if was_sprinting && wish.is_none() && self.speed > RUN_SPEED + 0.5 {
            self.start(ActionId::SprintStop);
            return true;
        }

        if inp.guard.held && !self.sprinting && self.left_button() == LeftButton::Guard {
            self.guarding = true;
            self.guard_t += DF;
        } else {
            self.guarding = false;
            self.guard_t = 0.0;
        }

        let strafing = target.is_some() && !self.sprinting;
        let target_speed = match wish {
            None => 0.0,
            Some(_) if self.sprinting => SPRINT_SPEED,
            Some(_) if self.crouching && (inp.walk || inp.tilt() < WALK_TILT) => CROUCH_WALK_SPEED,
            Some(_) if self.crouching => CROUCH_RUN_SPEED,
            Some(_) if inp.walk || inp.tilt() < WALK_TILT => WALK_SPEED,
            // Backpedalling and sidestepping around a target are separate, slower loops.
            Some(w) if strafing => {
                let along = w.dot(self.facing());
                if along > FRAC_PI_4.cos() {
                    RUN_SPEED
                } else if along < -FRAC_PI_4.cos() {
                    RUN_BACK_SPEED
                } else {
                    RUN_SIDE_SPEED
                }
            }
            Some(_) => RUN_SPEED,
        };
        let rate = if target_speed > self.speed { ACCEL } else { DECEL };
        self.speed = approach(self.speed, target_speed, rate * DT);

        match target {
            // Locked on: strafe around the target, facing it.
            Some(t) if strafing => {
                if let Some(goal) = self.yaw_to(t) {
                    self.yaw = turn_toward(self.yaw, goal, TURN_LOCKED.to_radians() * DT);
                }
                if let Some(w) = wish {
                    self.move_dir = w;
                }
            }
            // Otherwise the character runs where it faces and steers toward the stick.
            _ => {
                if let Some(w) = wish {
                    let rate = if self.sprinting { TURN_SPRINT } else { TURN_RUN };
                    self.yaw = turn_toward(self.yaw, yaw_of(w), rate.to_radians() * DT);
                    self.move_dir = self.facing();
                }
            }
        }

        level.slide(&mut self.pos, self.move_dir * self.speed * DT);
        if !self.follow_ground(level, self.move_dir * self.speed) {
            return false;
        }
        !self.sprinting
    }

    fn act(&mut self, mut a: Act, inp: &Input, level: &Level, target: Option<Vec3>) -> bool {
        let def = a.id.def();
        let wish = inp.wish();
        let prev = a.f;

        // Letting go inside the charge window swaps to the uncharged swing.
        if let Some((from, to)) = def.charge {
            if a.f >= from && a.f < to && !inp.heavy.held {
                if let ActionId::Attack(moveset, kind) = a.id {
                    let release = match kind {
                        AttackKind::Heavy2Charge => AttackKind::Heavy2,
                        _ => AttackKind::Heavy1,
                    };
                    if moveset.has(release) {
                        self.start(ActionId::Attack(moveset, release));
                        return false;
                    }
                }
            }
        }
        a.f += DF;

        // An attack's stamina is taken as each hit comes out, not when it starts.
        for (i, hit) in def.hits.iter().enumerate() {
            if a.paid & (1 << i) == 0 && a.f >= hit.from {
                self.spend(hit.stamina);
                a.paid |= 1 << i;
            }
        }

        // Locked-on dodges keep the facing they were launched with.
        let fixed = target.is_some()
            && matches!(a.id, ActionId::Roll(..) | ActionId::CrouchRoll(..) | ActionId::Backstep);
        if def.can_turn(a.f) && !fixed {
            let goal = match target {
                Some(t) => self.yaw_to(t),
                None => wish.map(yaw_of),
            };
            if let Some(goal) = goal {
                self.yaw = turn_toward(self.yaw, goal, def.turn_rate(a.f).to_radians() * DT);
            }
        }

        let (m0, m1) = (def.motion_at(prev), def.motion_at(a.f));
        let step = self.facing() * (m1[2] - m0[2]) + self.left() * (m1[0] - m0[0]);
        level.slide(&mut self.pos, step);

        if matches!(a.id, ActionId::Jump(_)) {
            self.jump(a, &def, step, inp, level, target.is_some());
            return false;
        }
        if !self.follow_ground(level, (step / DT).clamp_length_max(SPRINT_SPEED)) {
            return false;
        }

        if let Some(req) = self.buffer {
            let open = a.f
                >= match req {
                    Req::Light => def.cancel_light,
                    Req::Heavy => def.cancel_heavy,
                    Req::Left => def.cancel_left,
                    Req::Dodge => def.cancel_dodge,
                    Req::Jump => def.cancel_jump,
                };
            if open {
                self.buffer = None;
                if self.stamina > 0.0 {
                    let next = match req {
                        Req::Dodge => {
                            self.start_dodge(wish, target);
                            return false;
                        }
                        Req::Jump => {
                            self.start_jump(inp, target);
                            return false;
                        }
                        Req::Light => self.next_light(a.id),
                        Req::Heavy => self.next_heavy(a.id),
                        Req::Left => self.next_left(a.id),
                    };
                    if let Some(next) = next {
                        self.start(next);
                        return false;
                    }
                }
            }
        }

        let regen = a.f >= def.cancel_move;
        if inp.guard.held && a.f >= def.cancel_guard && self.left_button() == LeftButton::Guard {
            self.state = State::Ground;
            self.speed = 0.0;
        } else if let (Some(w), true) = (wish, a.f >= def.cancel_move) {
            self.state = State::Ground;
            self.move_dir = w;
            self.speed = match a.id {
                ActionId::LandSprint if !self.sprint_held(inp) => RUN_SPEED,
                id => id.exit_speed(),
            };
        } else if a.f >= def.total {
            self.state = State::Ground;
            self.speed = 0.0;
        } else {
            self.state = State::Act(a);
        }
        regen
    }

    /// The jump is an authored arc, not physics: height comes from the
    /// animation until it ends, and only then does gravity take over.
    fn jump(&mut self, a: Act, def: &ActionDef, step: Vec3, inp: &Input, level: &Level, locked: bool) {
        let up = def.motion_at(a.f)[1];
        let ground = level.height(self.pos.x, self.pos.z);
        self.try_air_attack(a.f >= def.cancel_light);

        if up <= 0.0 {
            // Still crouching into the jump. The wind-up carries the character
            // forward, possibly past a ledge: the jump then leaves from the
            // height it started at instead of dropping to the ground below.
            if self.pos.y - ground <= STEP_HEIGHT {
                self.pos.y = ground;
            }
            self.jump_base = self.pos.y;
            self.peak = self.pos.y;
            self.state = State::Act(a);
            return;
        }
        let y = self.jump_base + up;
        self.peak = self.peak.max(y);
        if y <= ground {
            self.land(ground, inp, true, locked);
            return;
        }
        self.pos.y = y;
        if a.f < def.total {
            self.state = State::Act(a);
            return;
        }
        // Hand over to the fall with the arc's exit velocity.
        let n = def.motion.len();
        let rise = (def.motion[n - 1][1] - def.motion[n - 2][1]) * ANIM_FPS;
        let flat = step / DT;
        self.state = State::Air(Air { vel: Vec3::new(flat.x, rise, flat.z), jumped: true, f: 0.0 });
    }

    fn air(&mut self, mut a: Air, inp: &Input, level: &Level, locked: bool) {
        a.f += DF;
        a.vel.y = (a.vel.y - GRAVITY * DT).max(-TERMINAL_VELOCITY);
        level.slide(&mut self.pos, Vec3::new(a.vel.x, 0.0, a.vel.z) * DT);
        self.pos.y += a.vel.y * DT;
        self.peak = self.peak.max(self.pos.y);
        // Jump attacks never come out of a plain fall.
        self.try_air_attack(a.jumped);

        let ground = level.height(self.pos.x, self.pos.z);
        if a.vel.y > 0.0 || self.pos.y > ground {
            self.state = State::Air(a);
        } else {
            self.land(ground, inp, a.jumped && !a.falling(), locked);
        }
    }

    /// Starts a jump attack from a queued press, once per jump.
    fn try_air_attack(&mut self, allowed: bool) {
        if !allowed || self.air_attack.is_some() || self.stamina <= 0.0 {
            return;
        }
        let (heavy, paired) = match self.buffer {
            Some(Req::Light) => (false, false),
            Some(Req::Heavy) => (true, false),
            Some(Req::Left) if self.paired() => (false, true),
            _ => return,
        };
        self.buffer = None;
        let moveset = self.moveset();
        let Some(def) = (if paired { moveset.air_paired() } else { moveset.air(heavy) }) else {
            return;
        };
        self.spend(def.stamina);
        self.air_attack = Some(AirAttack { heavy, paired, moveset, def, f: 0.0, hit_done: false });
    }

    fn land(&mut self, ground: f32, inp: &Input, jumped: bool, locked: bool) {
        self.pos.y = ground;
        let fall = self.peak - ground;
        let attack = self.air_attack.take();
        if fall >= FALL_DEATH {
            self.die();
            return;
        }
        if fall >= FALL_DAMAGE_START {
            let t = (fall - FALL_DAMAGE_START) / (FALL_DEATH - FALL_DAMAGE_START);
            self.hp -= MAX_HP * (0.3 + 0.6 * t);
            if self.hp <= 0.0 {
                self.die();
                return;
            }
        }

        if let Some(attack) = attack {
            let (landing, short) = if attack.paired {
                (AttackKind::PairedJumpLand, AttackKind::PairedJumpLandShort)
            } else if attack.heavy {
                (AttackKind::JumpHeavyLand, AttackKind::JumpHeavyLandShort)
            } else {
                (AttackKind::JumpLightLand, AttackKind::JumpLightLandShort)
            };
            let finished = attack.f >= attack.def.to;
            let kind = if finished && attack.moveset.has(short) { short } else { landing };
            let Some(def) = attack.moveset.attack(kind) else {
                self.start(ActionId::LandLight);
                return;
            };
            self.start(ActionId::Attack(attack.moveset, kind));
            if let State::Act(act) = &mut self.state {
                // Still coming down: the landing animation carries the hit.
                // Already swung: it must not hit a second time.
                act.f = if finished { 0.0 } else { attack.f.min(def.hit().map_or(0.0, |hit| hit.from)) };
                // The swing was paid for in the air.
                act.paid = u32::MAX;
                act.landed = if attack.hit_done || finished { u32::MAX } else { 0 };
            }
        } else if fall >= FALL_HEAVY_LANDING {
            self.start(ActionId::LandHeavy);
        } else if !jumped {
            self.start(ActionId::LandFall);
        } else if let Some(w) = inp.wish() {
            // Landing with the stick held runs straight out of the jump.
            if locked && !self.sprint_held(inp) {
                // Locked on, that is a step in one of four directions, still facing the target.
                let side = self.square_up(yaw_of(w), true);
                let walking = inp.walk || inp.tilt() < WALK_TILT;
                self.start(if walking { ActionId::LandStrafeWalk(side) } else { ActionId::LandStrafe(side) });
                return;
            }
            self.yaw = yaw_of(w);
            self.start(if self.sprint_held(inp) { ActionId::LandSprint } else { ActionId::LandRun });
        } else {
            self.start(ActionId::LandLight);
        }
    }

    /// Keeps a grounded character on the floor. Returns false if it walked off
    /// a ledge and is now falling with `carry` as its velocity.
    fn follow_ground(&mut self, level: &Level, carry: Vec3) -> bool {
        let ground = level.height(self.pos.x, self.pos.z);
        if self.pos.y - ground > STEP_HEIGHT {
            self.sprinting = false;
            self.guarding = false;
            self.crouching = false;
            self.peak = self.pos.y;
            self.state = State::Air(Air { vel: Vec3::new(carry.x, 0.0, carry.z), jumped: false, f: 0.0 });
            return false;
        }
        self.pos.y = ground;
        true
    }

    fn yaw_to(&self, point: Vec3) -> Option<f32> {
        let to = Vec3::new(point.x - self.pos.x, 0.0, point.z - self.pos.z);
        (to.length_squared() > 1e-4).then(|| yaw_of(to))
    }

    fn spend(&mut self, cost: f32) {
        self.stamina = (self.stamina - cost).max(0.0);
    }

    fn start(&mut self, id: ActionId) {
        self.spend(id.start_cost());
        self.state = State::Act(Act { id, f: 0.0, landed: 0, paid: 0 });
        self.speed = 0.0;
        self.sprinting = false;
        self.crouching &= matches!(id, ActionId::CrouchRoll(..));
        self.guarding = false;
        if id != ActionId::GuardHit {
            self.guard_t = 0.0;
        }
    }

    /// With a direction this is a roll that way; without one, a backstep.
    fn start_dodge(&mut self, wish: Option<Vec3>, target: Option<Vec3>) {
        let Some(dir) = wish else {
            self.start(ActionId::Backstep);
            return;
        };
        let side = self.square_up(yaw_of(dir), target.is_some());
        self.start(if self.crouching { ActionId::CrouchRoll(self.load, side) } else { ActionId::Roll(self.load, side) });
    }

    /// Turns the character for a move toward `goal`. Free, it simply faces
    /// that way. Locked on, it picks the nearest of front, back, left and
    /// right and squares up so that direction points exactly at `goal`, which
    /// keeps it facing roughly at its target. Returns the direction picked.
    fn square_up(&mut self, goal: f32, locked: bool) -> Dir {
        let off = angle_diff(self.yaw, goal);
        let side = if !locked || off.abs() <= FRAC_PI_4 {
            Dir::Front
        } else if off.abs() >= PI - FRAC_PI_4 {
            Dir::Back
        } else if off > 0.0 {
            Dir::Left
        } else {
            Dir::Right
        };
        self.yaw = match side {
            Dir::Front => goal,
            Dir::Back => goal + PI,
            Dir::Left => goal - FRAC_PI_2,
            Dir::Right => goal + FRAC_PI_2,
        };
        side
    }

    fn start_jump(&mut self, inp: &Input, target: Option<Vec3>) {
        let kind = match inp.wish() {
            None => JumpKind::Stand,
            Some(dir) if self.sprinting => {
                self.yaw = yaw_of(dir);
                JumpKind::Sprint
            }
            Some(dir) => {
                // Locked on, walking and running jumps go four ways like rolls do.
                let side = self.square_up(yaw_of(dir), target.is_some());
                JumpKind::toward(side, inp.walk || inp.tilt() < WALK_TILT)
            }
        };
        self.jump_base = self.pos.y;
        self.peak = self.pos.y;
        self.air_attack = None;
        self.start(ActionId::Jump(kind));
    }

    fn next_light(&self, from: ActionId) -> Option<ActionId> {
        use AttackKind::*;
        match from {
            // The game's script sends a light press after any of these openers
            // into the *second* swing of the chain, not the first.
            ActionId::Attack(_, RunLight | RollAttack | BackstepAttack | CrouchAttack) => self.pick(&[Light2, Light1]),
            ActionId::Attack(_, kind) => {
                let next = kind.next_light().unwrap_or(Light1);
                // Chains differ in length per weapon; past the end they start over.
                self.pick(&[next, Light1])
            }
            ActionId::Roll(..) | ActionId::CrouchRoll(..) => self.pick(&[RollAttack, Light1]),
            ActionId::Backstep => self.pick(&[BackstepAttack, Light1]),
            ActionId::SprintStop => self.pick(&[RunLight, Light1]),
            _ => self.pick(&[Light1]),
        }
    }

    fn next_left(&self, from: ActionId) -> Option<ActionId> {
        use AttackKind::*;
        let (paired, single) = match from {
            ActionId::Attack(_, kind) => (kind.next_paired().unwrap_or(PairedLight1), kind.next_left().unwrap_or(LeftLight1)),
            ActionId::Roll(..) | ActionId::CrouchRoll(..) => (PairedRoll, LeftLight1),
            ActionId::Backstep => (PairedBackstep, LeftLight1),
            ActionId::SprintStop => (PairedRun, LeftLight1),
            _ => (PairedLight1, LeftLight1),
        };
        // Past the end of a chain it starts over.
        self.pick_left(&[paired, PairedLight1], &[single, LeftLight1])
    }

    fn next_heavy(&self, from: ActionId) -> Option<ActionId> {
        use AttackKind::*;
        match from {
            _ if self.guard_counter_ready() => self.pick(&[GuardCounter, Heavy1Charge]),
            ActionId::Attack(_, Heavy1 | Heavy1Charge) => self.pick(&[Heavy2Charge, Heavy1Charge]),
            ActionId::SprintStop => self.pick(&[RunHeavy, Heavy1Charge]),
            _ => self.pick(&[Heavy1Charge, Heavy1]),
        }
    }

    pub fn receive_hit(&mut self, hit: &Incoming, level: &Level) -> HitResult {
        if self.is_dead() {
            return HitResult::Ignored;
        }
        if self.invincible() {
            return HitResult::Dodged;
        }
        let airborne = self.airborne();
        if hit.low && airborne {
            let clearance = self.pos.y - level.height(self.pos.x, self.pos.z);
            if clearance > JUMP_CLEARANCE {
                return HitResult::Jumped;
            }
        }

        let toward = Vec3::new(hit.from.x - self.pos.x, 0.0, hit.from.z - self.pos.z);
        let in_arc = toward.normalize_or_zero().dot(self.facing()) >= GUARD_ARC_DEG.to_radians().cos();
        if self.guard_up() && in_arc {
            self.stamina -= hit.stamina * GUARD_STAMINA_TAKEN;
            if self.stamina <= 0.0 {
                self.stamina = 0.0;
                self.start(ActionId::GuardBreak);
                return HitResult::GuardBroken;
            }
            self.start(ActionId::GuardHit);
            self.guard_counter = GUARD_COUNTER_WINDOW;
            return HitResult::Blocked;
        }

        // Being hit knocks a grip change out of the hands.
        self.swap = None;
        self.hp -= hit.damage;
        if self.hp <= 0.0 {
            self.die();
            return HitResult::Killed;
        }
        if !airborne {
            // React according to the side the hit landed on.
            let off = angle_diff(self.yaw, yaw_of(toward));
            let side = if toward.length_squared() < 1e-6 || off.abs() <= FRAC_PI_4 {
                Dir::Front
            } else if off.abs() >= PI - FRAC_PI_4 {
                Dir::Back
            } else if off > 0.0 {
                Dir::Left
            } else {
                Dir::Right
            };
            self.start(ActionId::Hurt(hit.level, side));
        }
        HitResult::Hit
    }

    fn die(&mut self) {
        self.hp = 0.0;
        self.sprinting = false;
        self.guarding = false;
        self.crouching = false;
        self.buffer = None;
        self.air_attack = None;
        self.swap = None;
        self.state = State::Dead { t: 0.0 };
    }
}

/// What the left hand takes up next: the shield, then nothing, then a torch,
/// then each weapon in turn.
pub fn next_left_hand(left: usize) -> usize {
    let order: Vec<usize> = [SHIELD, FIST, TORCH].into_iter().chain((0..SHIELD).filter(|&i| i != FIST && i != TORCH)).collect();
    let at = order.iter().position(|&i| i == left).unwrap_or(0);
    order[(at + 1) % order.len()]
}
