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
    Dodge,
    Jump,
}

#[derive(Clone, Copy, Debug)]
pub struct Act {
    pub id: ActionId,
    pub f: f32,
    pub hit_done: bool,
    /// The hit's stamina cost has been taken.
    paid: bool,
}

/// A jump attack in progress. It runs on its own clock over the jump or fall.
#[derive(Clone, Copy, Debug)]
pub struct AirAttack {
    pub heavy: bool,
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
    pub range: f32,
    pub half_angle: f32,
}

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
    pub grip: Grip,
    pub in_combat: bool,
    pub buffer: Option<Req>,
    pub air_attack: Option<AirAttack>,
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
            grip: Grip::OneHand,
            in_combat: false,
            buffer: None,
            air_attack: None,
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
            Grip::TwoHandLeft => Moveset { weapon: SHIELD as u8, two_hand: true },
        }
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
        let build = |id: ActionId, moveset: Moveset, hit: Hit| {
            let (range, half_angle) = id.reach();
            ActiveHit { attack: moveset.info().attack, mv: hit.mv, guard_damage: hit.guard_damage, range, half_angle }
        };
        if let Some(attack) = self.air_attack {
            let kind = if attack.heavy { AttackKind::JumpHeavyLand } else { AttackKind::JumpLightLand };
            let id = ActionId::Attack(attack.moveset, kind);
            let live = !attack.hit_done && attack.f >= attack.def.from && attack.f < attack.def.to;
            // The airborne swing uses the same hit as its landing follow-through.
            let hit = attack.moveset.attack(kind).and_then(|def| def.hit)?;
            return live.then(|| build(id, attack.moveset, hit));
        }
        match self.state {
            State::Act(a) if !a.hit_done => {
                let ActionId::Attack(moveset, _) = a.id else {
                    return None;
                };
                let hit = a.id.def().hit?;
                (a.f >= hit.from && a.f < hit.to).then(|| build(a.id, moveset, hit))
            }
            _ => None,
        }
    }

    pub fn mark_hit(&mut self) {
        if let Some(attack) = &mut self.air_attack {
            attack.hit_done = true;
        } else if let State::Act(a) = &mut self.state {
            a.hit_done = true;
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
        self.guard_counter = (self.guard_counter - DF).max(0.0);
        if let Some(attack) = &mut self.air_attack {
            attack.f += DF;
        }

        let regen = match self.state {
            State::Ground => self.ground(inp, level, target),
            State::Act(a) => self.act(a, inp, level, target),
            State::Air(a) => {
                self.air(a, inp, level);
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
            if inp.light.pressed {
                self.buffer = Some(Req::Light);
            }
        }
    }

    fn sprint_held(&self, inp: &Input) -> bool {
        inp.dodge.held && self.dodge_hold >= SPRINT_HOLD_FRAMES
    }

    fn ground(&mut self, inp: &Input, level: &Level, target: Option<Vec3>) -> bool {
        let wish = inp.wish();

        if let Some(req) = self.buffer.take() {
            if self.stamina > 0.0 {
                use AttackKind::*;
                let attack = match req {
                    Req::Dodge => {
                        self.start_dodge(wish, target);
                        return false;
                    }
                    Req::Jump => {
                        self.start_jump(inp);
                        return false;
                    }
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

        // Changing grip or weapon only happens from a neutral stance.
        if inp.two_hand_right {
            self.grip = if self.grip == Grip::TwoHandRight { Grip::OneHand } else { Grip::TwoHandRight };
        }
        if inp.two_hand_left {
            self.grip = if self.grip == Grip::TwoHandLeft { Grip::OneHand } else { Grip::TwoHandLeft };
        }
        if inp.next_weapon {
            // The shield is the last entry and stays in the left hand.
            self.weapon = (self.weapon + 1) % SHIELD;
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

        if inp.guard.held && !self.sprinting {
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

        // An attack's stamina is taken as its hit comes out, not when it starts.
        if let Some(hit) = def.hit {
            if !a.paid && a.f >= hit.from {
                self.spend(def.stamina);
                a.paid = true;
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
            self.jump(a, &def, step, inp, level);
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
                            self.start_jump(inp);
                            return false;
                        }
                        Req::Light => self.next_light(a.id),
                        Req::Heavy => self.next_heavy(a.id),
                    };
                    if let Some(next) = next {
                        self.start(next);
                        return false;
                    }
                }
            }
        }

        let regen = a.f >= def.cancel_move;
        if inp.guard.held && a.f >= def.cancel_guard {
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
    fn jump(&mut self, a: Act, def: &ActionDef, step: Vec3, inp: &Input, level: &Level) {
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
            self.land(ground, inp, true);
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

    fn air(&mut self, mut a: Air, inp: &Input, level: &Level) {
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
            self.land(ground, inp, a.jumped && !a.falling());
        }
    }

    /// Starts a jump attack from a queued press, once per jump.
    fn try_air_attack(&mut self, allowed: bool) {
        if !allowed || self.air_attack.is_some() || self.stamina <= 0.0 {
            return;
        }
        let heavy = match self.buffer {
            Some(Req::Light) => false,
            Some(Req::Heavy) => true,
            _ => return,
        };
        self.buffer = None;
        let moveset = self.moveset();
        let Some(def) = moveset.air(heavy) else {
            return;
        };
        self.spend(def.stamina);
        self.air_attack = Some(AirAttack { heavy, moveset, def, f: 0.0, hit_done: false });
    }

    fn land(&mut self, ground: f32, inp: &Input, jumped: bool) {
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
            let (landing, short) = if attack.heavy {
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
                act.f = if finished { 0.0 } else { attack.f.min(def.hit.map_or(0.0, |hit| hit.from)) };
                act.paid = true;
                act.hit_done = attack.hit_done || finished;
            }
        } else if fall >= FALL_HEAVY_LANDING {
            self.start(ActionId::LandHeavy);
        } else if !jumped {
            self.start(ActionId::LandFall);
        } else if let Some(w) = inp.wish() {
            // Landing with the stick held runs straight out of the jump.
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
        self.state = State::Act(Act { id, f: 0.0, hit_done: false, paid: false });
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
        let goal = yaw_of(dir);
        let side = if target.is_some() {
            // Locked on: pick the directional roll nearest the stick, then
            // square the character up so that roll travels exactly that way.
            let off = angle_diff(self.yaw, goal);
            if off.abs() <= FRAC_PI_4 {
                Dir::Front
            } else if off.abs() >= PI - FRAC_PI_4 {
                Dir::Back
            } else if off > 0.0 {
                Dir::Left
            } else {
                Dir::Right
            }
        } else {
            Dir::Front
        };
        self.yaw = match side {
            Dir::Front => goal,
            Dir::Back => goal + PI,
            Dir::Left => goal - FRAC_PI_2,
            Dir::Right => goal + FRAC_PI_2,
        };
        self.start(if self.crouching { ActionId::CrouchRoll(self.load, side) } else { ActionId::Roll(self.load, side) });
    }

    fn start_jump(&mut self, inp: &Input) {
        let kind = match inp.wish() {
            None => JumpKind::Stand,
            Some(dir) => {
                self.yaw = yaw_of(dir);
                if self.sprinting {
                    JumpKind::Sprint
                } else if inp.walk || inp.tilt() < WALK_TILT {
                    JumpKind::Walk
                } else {
                    JumpKind::Run
                }
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
        self.state = State::Dead { t: 0.0 };
    }
}
