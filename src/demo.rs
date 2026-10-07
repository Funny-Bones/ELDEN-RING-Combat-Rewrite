//! A scripted tour of everything the sandbox does, for recording. Press Enter
//! to start it (or to stop it early).
//!
//! The `Director` plays the character through the ordinary input struct, so
//! what it shows is exactly what a player could do. It steers by where the
//! character actually is rather than by a fixed timeline, which keeps it on
//! course whatever the frame rate.

use bevy::math::{Vec2, Vec3};
use bevy::prelude::{ButtonInput, KeyCode, Res, ResMut, Resource};

use crate::camera::CamRig;
use crate::sim::data::*;
use crate::sim::dummy::{self, DState};
use crate::sim::level::Level;
use crate::sim::player::{Button, Input, State};
use crate::sim::{dir_of, yaw_of, World};
use crate::{Options, Pending, Sim};

#[derive(Clone, Copy, PartialEq)]
enum Gait {
    Walk,
    Run,
    Sprint,
}

#[derive(Clone, Copy, PartialEq)]
enum Btn {
    Dodge,
    Jump,
    Light,
    Heavy,
    Guard,
}

#[derive(Clone, Copy)]
enum Key {
    Crouch,
    Lock,
    TwoHandRight,
    TwoHandLeft,
    NextWeapon,
    NextLeft,
}

#[derive(Clone, Copy)]
enum Lean {
    Centre,
    /// A direction on the ground, (x, z).
    World(f32, f32),
    /// Relative to the camera, (right, forward).
    Cam(f32, f32),
}

#[derive(Clone, Copy)]
enum Cond {
    /// Standing on the ground with nothing in progress.
    Free,
    /// The dummy is this many seconds into winding up a swing.
    Windup(f32),
    Hurt,
    Blocked,
    /// The current action has reached this frame.
    Frame(f32),
    Dead,
}

#[derive(Clone, Copy)]
enum Step {
    /// Caption on screen.
    Say(&'static str),
    Wait(u32),
    /// Swing the free camera round to this yaw.
    Cam(f32),
    /// Whether `Go` turns the camera to look along the way.
    Follow(bool),
    /// Move to this spot on the ground, (x, z).
    Go(f32, f32, Gait),
    /// Close in on, or back away from, the dummy until this far from it.
    Range(f32),
    /// Sprint at the dummy until this close.
    Charge(f32),
    /// Hold the stick like this until told otherwise.
    Stick(Lean, Gait),
    Tap(Btn),
    Hold(Btn, bool),
    Press(Key),
    /// Press until the attack actually comes out, so presses that fall
    /// outside an animation's input window are simply repeated.
    Attack(Btn),
    /// Wait for the next attack to come out without pressing anything, as
    /// when a charge is let go and becomes the swing.
    Swing,
    Until(Cond),
    Do(fn(&mut World)),
}

pub struct Director {
    steps: Vec<Step>,
    at: usize,
    /// Ticks spent on the current step.
    t: u32,
    pub caption: &'static str,
    /// Yaw the free camera should ease toward.
    pub cam: f32,
    follow: bool,
    stick: Lean,
    gait: Gait,
    held: [bool; 5],
    was_held: [bool; 5],
    /// The action seen last tick, to notice a new attack starting.
    last: (Option<ActionId>, f32, bool),
}

/// Ticks a step may take before the tour moves on regardless.
const PATIENCE: u32 = 1800;

impl Director {
    pub fn new() -> Self {
        Self {
            steps: script(),
            at: 0,
            t: 0,
            caption: "",
            cam: 0.0,
            follow: true,
            stick: Lean::Centre,
            gait: Gait::Run,
            held: [false; 5],
            was_held: [false; 5],
            last: (None, 0.0, false),
        }
    }

    /// An attack has started since last tick.
    fn fresh_attack(&self, now: (Option<ActionId>, f32, bool)) -> bool {
        let attacking = matches!(now.0, Some(ActionId::Attack(..)));
        attacking && (now.0 != self.last.0 || now.1 < self.last.1) || (now.2 && !self.last.2)
    }

    /// The input for this tick, or `None` once the tour is over.
    pub fn drive(&mut self, world: &mut World, cam_yaw: f32) -> Option<Input> {
        let mut stick = self.stick;
        let mut gait = self.gait;
        let mut held = self.held;
        let mut key = None;

        let acting = match world.player.state {
            State::Act(a) => Some((a.id, a.f)),
            _ => None,
        };
        let now = (acting.map(|a| a.0), acting.map_or(0.0, |a| a.1), world.player.air_attack.is_some());

        loop {
            let step = *self.steps.get(self.at)?;
            let t = self.t;
            let p = &world.player;
            let to_dummy = Vec3::new(world.dummy.pos.x - p.pos.x, 0.0, world.dummy.pos.z - p.pos.z);
            let done = match step {
                Step::Say(caption) => {
                    self.caption = caption;
                    true
                }
                Step::Cam(yaw) => {
                    self.cam = yaw;
                    true
                }
                Step::Follow(follow) => {
                    self.follow = follow;
                    true
                }
                Step::Stick(s, g) => {
                    (self.stick, self.gait) = (s, g);
                    (stick, gait) = (s, g);
                    true
                }
                Step::Hold(button, on) => {
                    self.held[button as usize] = on;
                    held[button as usize] = on;
                    true
                }
                Step::Do(change) => {
                    change(world);
                    true
                }
                Step::Wait(ticks) => t >= ticks,
                Step::Go(x, z, g) => {
                    let to = Vec3::new(x - p.pos.x, 0.0, z - p.pos.z);
                    if to.length() < 0.5 {
                        true
                    } else {
                        if self.follow && t == 0 {
                            self.cam = yaw_of(to);
                        }
                        (stick, gait) = (Lean::World(to.x, to.z), g);
                        false
                    }
                }
                Step::Range(distance) => {
                    let off = to_dummy.length() - distance;
                    if (-0.5..=0.3).contains(&off) {
                        true
                    } else {
                        let toward = to_dummy * off.signum();
                        (stick, gait) = (Lean::World(toward.x, toward.z), Gait::Run);
                        false
                    }
                }
                Step::Charge(distance) => {
                    if to_dummy.length() <= distance {
                        true
                    } else {
                        (stick, gait) = (Lean::World(to_dummy.x, to_dummy.z), Gait::Sprint);
                        false
                    }
                }
                Step::Tap(button) => {
                    // The dodge button is read on release, so it needs a moment held.
                    let ticks = if button == Btn::Dodge { 3 } else { 2 };
                    held[button as usize] = t + 1 < ticks;
                    t >= ticks
                }
                Step::Press(k) => {
                    if t == 0 {
                        key = Some(k);
                    }
                    t >= 1
                }
                Step::Attack(button) => {
                    let started = t > 0 && self.fresh_attack(now);
                    held[button as usize] = !started && t % 6 == 0;
                    started || t > 150
                }
                Step::Swing => t > 0 && self.fresh_attack(now) || t > 150,
                Step::Until(what) => {
                    let ok = match what {
                        Cond::Free => matches!(p.state, State::Ground) && p.swap.is_none() && p.air_attack.is_none(),
                        Cond::Windup(secs) => {
                            matches!(world.dummy.state, DState::Windup { .. }) && world.dummy.t >= secs
                        }
                        Cond::Hurt => matches!(now.0, Some(ActionId::Hurt(..))),
                        Cond::Blocked => now.0 == Some(ActionId::GuardHit),
                        Cond::Frame(frame) => now.0.is_none() || now.1 >= frame,
                        Cond::Dead => p.is_dead(),
                    };
                    ok || t > PATIENCE
                }
            };
            if done || t > PATIENCE {
                self.at += 1;
                self.t = 0;
            } else {
                self.t += 1;
                break;
            }
        }
        self.last = now;

        let forward = dir_of(cam_yaw);
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        let mv = match stick {
            Lean::Centre => Vec2::ZERO,
            Lean::Cam(x, y) => Vec2::new(x, y),
            Lean::World(x, z) => {
                let dir = Vec3::new(x, 0.0, z).normalize_or_zero();
                Vec2::new(dir.dot(right), dir.dot(forward))
            }
        };
        if gait == Gait::Sprint && mv != Vec2::ZERO {
            held[Btn::Dodge as usize] = true;
        }
        let button = |b: Btn| {
            let (is, was) = (held[b as usize], self.was_held[b as usize]);
            Button { held: is, pressed: is && !was, released: was && !is }
        };
        let input = Input {
            mv,
            cam_yaw,
            dodge: button(Btn::Dodge),
            jump: button(Btn::Jump),
            light: button(Btn::Light),
            heavy: button(Btn::Heavy),
            guard: button(Btn::Guard),
            walk: gait == Gait::Walk,
            crouch: matches!(key, Some(Key::Crouch)),
            lock: matches!(key, Some(Key::Lock)),
            two_hand_right: matches!(key, Some(Key::TwoHandRight)),
            two_hand_left: matches!(key, Some(Key::TwoHandLeft)),
            next_weapon: matches!(key, Some(Key::NextWeapon)),
            next_left: matches!(key, Some(Key::NextLeft)),
        };
        self.was_held = held;
        Some(input)
    }
}

fn refresh(world: &mut World) {
    world.player.hp = MAX_HP;
    world.player.stamina = MAX_STAMINA;
    if world.dummy.alive() {
        world.dummy.hp = dummy::MAX_HP;
    }
}

/// Puts the named weapon in both hands.
fn pair(world: &mut World, name: &str) {
    let weapon = WEAPONS.iter().position(|info| info.name == name).unwrap();
    world.player.weapon = weapon;
    world.player.left = weapon;
}

fn hostile(world: &mut World) {
    world.dummy.aggressive = true;
}

fn calm(world: &mut World) {
    world.dummy.aggressive = false;
}

/// The whole tour, in order.
fn script() -> Vec<Step> {
    use Step::*;
    const FORWARD: Lean = Lean::Cam(0.0, 1.0);
    const BACK: Lean = Lean::Cam(0.0, -1.0);
    const LEFT: Lean = Lean::Cam(-1.0, 0.0);
    const RIGHT: Lean = Lean::Cam(1.0, 0.0);
    let free = Until(Cond::Free);
    let stop = Stick(Lean::Centre, Gait::Run);
    let mut s = vec![Wait(60)];

    // --- Getting about ------------------------------------------------------
    s.extend([
        Say("Walk"),
        Go(3.0, -3.0, Gait::Walk),
        Say("Run"),
        Go(-5.0, -4.0, Gait::Run),
        Say("Stopping from a run"),
        Wait(50),
        Say("Sprint"),
        Go(6.5, -5.5, Gait::Sprint),
        Wait(50),
        Say("Slopes: each foot finds the ground"),
        Go(14.0, -5.5, Gait::Walk),
        Wait(30),
        Say("Walking off a ledge"),
        Go(14.0, -1.5, Gait::Run),
        free,
        Wait(20),
    ]);

    // --- Crouching ------------------------------------------------------------
    s.extend([
        Say("Crouch"),
        Go(8.0, -2.5, Gait::Run),
        Wait(30),
        Press(Key::Crouch),
        Wait(50),
        Say("Crouched walk and run"),
        Go(4.0, -3.0, Gait::Walk),
        Go(-3.0, -5.0, Gait::Run),
        Wait(50),
        Say("Crouched roll"),
        Stick(Lean::World(1.0, 0.0), Gait::Run),
        Tap(Btn::Dodge),
        stop,
        free,
        Wait(20),
        Say("Standing up"),
        Press(Key::Crouch),
        Wait(50),
    ]);

    // --- Rolls ------------------------------------------------------------------
    s.extend([Follow(false), Cam(0.0), Go(0.0, -5.0, Gait::Run), Wait(30)]);
    let loads: [(&str, &str, fn(&mut World), f32); 3] = [
        ("Roll: light equip load", "Backstep: light equip load", |w| w.player.load = Load::Light, 1.0),
        ("Roll: medium equip load", "Backstep: medium equip load", |w| w.player.load = Load::Medium, -1.0),
        ("Roll: heavy equip load", "Backstep: heavy equip load", |w| w.player.load = Load::Heavy, 1.0),
    ];
    for (roll, backstep, load, side) in loads {
        s.extend([
            Do(load),
            Say(roll),
            Stick(Lean::World(side, 0.0), Gait::Run),
            Tap(Btn::Dodge),
            stop,
            free,
            Wait(15),
            Say(backstep),
            Tap(Btn::Dodge),
            free,
            Wait(15),
        ]);
    }
    s.push(Do(|w| w.player.load = Load::Medium));

    // --- Jumps ------------------------------------------------------------------
    s.extend([
        Go(5.0, -3.0, Gait::Run),
        Wait(30),
        Say("Jump"),
        Tap(Btn::Jump),
        free,
        Wait(15),
        Say("Running jump"),
        Stick(Lean::World(-1.0, 0.0), Gait::Run),
        Wait(25),
        Tap(Btn::Jump),
        Wait(50),
        stop,
        free,
        Wait(30),
        Say("Sprinting jump"),
        Stick(Lean::World(1.0, 0.0), Gait::Sprint),
        Wait(45),
        Tap(Btn::Jump),
        Wait(50),
        stop,
        free,
        Wait(30),
    ]);

    // --- Climbing ---------------------------------------------------------------
    s.extend([
        Follow(true),
        Say("Jumping up onto things"),
        Go(-7.0, 1.8, Gait::Run),
        Cam(0.0),
        Stick(Lean::World(0.0, 1.0), Gait::Run),
        Wait(8),
        Tap(Btn::Jump),
        Wait(8),
        stop,
        free,
        Go(-6.7, 5.0, Gait::Walk),
        Cam(std::f32::consts::FRAC_PI_2 * -1.0),
        Stick(Lean::World(-1.0, 0.0), Gait::Run),
        Wait(6),
        Tap(Btn::Jump),
        Wait(8),
        stop,
        free,
        Go(-9.0, 5.0, Gait::Walk),
        Stick(Lean::World(-1.0, 0.0), Gait::Run),
        Wait(6),
        Tap(Btn::Jump),
        Wait(8),
        stop,
        free,
        Say("Along the top of the wall"),
        Go(-13.5, 5.0, Gait::Walk),
        Go(-13.5, 6.9, Gait::Run),
        Say("Across a gap"),
        Stick(Lean::World(0.0, 1.0), Gait::Run),
        Tap(Btn::Jump),
        Wait(10),
        stop,
        free,
        Wait(20),
        Say("And back down"),
        Go(-10.5, 12.0, Gait::Run),
        free,
        Wait(20),
    ]);

    // --- Lock-on ----------------------------------------------------------------
    s.extend([
        Go(0.0, 2.5, Gait::Run),
        Wait(20),
        Say("Lock-on"),
        Press(Key::Lock),
        Wait(50),
        Say("Lock-on: circling the target"),
        Stick(LEFT, Gait::Run),
        Wait(90),
        Stick(RIGHT, Gait::Run),
        Wait(110),
        Stick(BACK, Gait::Run),
        Wait(40),
        Stick(FORWARD, Gait::Run),
        Wait(40),
        stop,
        Wait(30),
        Range(5.0),
        Say("Lock-on: rolls in four directions"),
    ]);
    for dir in [LEFT, RIGHT, BACK, FORWARD] {
        s.extend([Stick(dir, Gait::Run), Tap(Btn::Dodge), stop, free, Wait(12)]);
    }
    s.extend([Range(5.0), Say("Lock-on: jumps in four directions")]);
    for dir in [LEFT, RIGHT, BACK, FORWARD] {
        s.extend([Stick(dir, Gait::Run), Wait(12), Tap(Btn::Jump), Wait(62), stop, free, Wait(12)]);
    }

    // --- Attacks ----------------------------------------------------------------
    s.extend([Do(refresh), Say("Light attack chain"), Range(1.7)]);
    s.extend([Attack(Btn::Light); 6]);
    s.extend([free, Wait(15), Do(refresh), Say("Heavy attacks"), Range(1.7), Attack(Btn::Heavy), Swing, Attack(Btn::Heavy), Swing, free]);
    s.extend([
        Wait(15),
        Do(refresh),
        Say("Charged heavy attack"),
        Range(1.9),
        Hold(Btn::Heavy, true),
        Wait(10),
        free,
        Hold(Btn::Heavy, false),
        Wait(15),
        Do(refresh),
        Say("Running attack"),
        Range(9.0),
        Charge(3.2),
        Attack(Btn::Light),
        stop,
        free,
        Wait(15),
        Say("Running heavy attack"),
        Range(9.0),
        Charge(3.6),
        Attack(Btn::Heavy),
        free,
        Wait(15),
        Do(refresh),
        Say("Rolling attack"),
        Range(5.5),
        Stick(FORWARD, Gait::Run),
        Tap(Btn::Dodge),
        stop,
        Attack(Btn::Light),
        free,
        Wait(15),
        Say("Backstep attack"),
        Range(1.6),
        Tap(Btn::Dodge),
        Attack(Btn::Light),
        free,
        Wait(15),
        Do(refresh),
        Say("Crouch attack"),
        Range(1.9),
        Press(Key::Crouch),
        Wait(45),
        Attack(Btn::Light),
        free,
        Wait(15),
        Say("Jump attack"),
        Range(4.0),
        Stick(FORWARD, Gait::Run),
        Wait(10),
        Tap(Btn::Jump),
        Attack(Btn::Light),
        stop,
        free,
        Wait(15),
        Do(refresh),
        Say("Heavy jump attack"),
        Range(4.0),
        Stick(FORWARD, Gait::Run),
        Wait(10),
        Tap(Btn::Jump),
        Attack(Btn::Heavy),
        stop,
        free,
        Wait(15),
    ]);

    // --- Grips ------------------------------------------------------------------
    s.extend([
        Do(refresh),
        Say("Two-handing the weapon"),
        Range(2.2),
        Press(Key::TwoHandRight),
        Wait(60),
        Range(1.7),
        Attack(Btn::Light),
        Attack(Btn::Light),
        Attack(Btn::Light),
        free,
        Attack(Btn::Heavy),
        free,
        Say("A two-handed weapon guards"),
        Hold(Btn::Guard, true),
        Wait(70),
        Hold(Btn::Guard, false),
        Wait(15),
        Do(refresh),
        Say("Two-handing the shield"),
        Range(2.2),
        Press(Key::TwoHandLeft),
        Wait(60),
        Range(1.4),
        Attack(Btn::Light),
        Attack(Btn::Light),
        free,
        Wait(15),
        Say("Back to one hand each"),
        Press(Key::TwoHandLeft),
        Wait(60),
    ]);

    // --- The left hand ----------------------------------------------------------
    s.extend([
        Do(refresh),
        Say("Off-hand: nothing, so it punches"),
        Range(2.2),
        Press(Key::NextLeft),
        Wait(60),
        Range(1.3),
        Attack(Btn::Guard),
        Attack(Btn::Guard),
        free,
        Say("Off-hand: torch"),
        Range(2.2),
        Press(Key::NextLeft),
        Wait(60),
        Range(1.4),
        Attack(Btn::Guard),
        Attack(Btn::Guard),
        free,
        Say("Off-hand: a dagger, with its own attacks"),
        Range(2.2),
        Press(Key::NextLeft),
        Wait(60),
        Range(1.4),
        Attack(Btn::Guard),
        Attack(Btn::Guard),
        Attack(Btn::Guard),
        free,
        Do(refresh),
        Say("The same weapon in each hand: paired attacks"),
        Range(2.2),
        Press(Key::NextLeft),
        Wait(60),
        Range(1.7),
        Attack(Btn::Guard),
        Attack(Btn::Guard),
        Attack(Btn::Guard),
        Attack(Btn::Guard),
        free,
        Wait(15),
        Do(refresh),
        Say("Paired running attack"),
        Range(9.0),
        Charge(3.4),
        Attack(Btn::Guard),
        stop,
        free,
        Wait(15),
        Say("Paired rolling attack"),
        Range(5.5),
        Stick(FORWARD, Gait::Run),
        Tap(Btn::Dodge),
        stop,
        Attack(Btn::Guard),
        free,
        Wait(15),
        Do(refresh),
        Say("Paired jump attack"),
        Range(4.0),
        Stick(FORWARD, Gait::Run),
        Wait(10),
        Tap(Btn::Jump),
        Attack(Btn::Guard),
        stop,
        free,
        Wait(15),
    ]);
    let pairs: [(&str, fn(&mut World)); 6] = [
        ("Paired greatswords", |w| pair(w, "Greatsword")),
        ("Paired twinblades", |w| pair(w, "Twinblade")),
        ("Paired great hammers", |w| pair(w, "Great Hammer")),
        ("Paired halberds", |w| pair(w, "Halberd")),
        ("Paired claws", |w| pair(w, "Claw")),
        ("Paired whips", |w| pair(w, "Whip")),
    ];
    for (caption, equip) in pairs {
        s.extend([Do(refresh), Range(2.4), Do(equip), Say(caption), Wait(30), Range(1.5), Attack(Btn::Guard), Attack(Btn::Guard), free]);
    }
    s.extend([
        Range(2.4),
        Do(|w| {
            w.player.weapon = DEFAULT_WEAPON;
            w.player.left = SHIELD;
        }),
        Say("Sword and shield again"),
        Wait(45),
    ]);

    // --- Every weapon class -----------------------------------------------------
    for i in 1..SHIELD {
        let weapon = (DEFAULT_WEAPON + i) % SHIELD;
        s.extend([
            Do(refresh),
            Say(WEAPONS[weapon].name),
            Range(2.4),
            Press(Key::NextWeapon),
            Wait(45),
            Range(1.5),
            Attack(Btn::Light),
            Attack(Btn::Light),
            free,
        ]);
    }
    s.extend([Say(WEAPONS[DEFAULT_WEAPON].name), Range(2.4), Press(Key::NextWeapon), Wait(60)]);

    // --- Defence ----------------------------------------------------------------
    // The dummy is only let off its leash for one swing at a time, so each of
    // its four attacks meets the answer meant for it.
    let face = |caption: &'static str| [Do(refresh), Range(2.8), Say(caption), Wait(30), Do(hostile)];
    s.extend(face("Rolling through an attack"));
    s.extend([
        Until(Cond::Windup(0.74)),
        Do(calm),
        Stick(FORWARD, Gait::Run),
        Tap(Btn::Dodge),
        stop,
        free,
        Wait(40),
    ]);
    s.extend(face("Jumping over a sweep"));
    s.extend([Until(Cond::Windup(0.62)), Do(calm), Tap(Btn::Jump), free, Wait(40)]);
    s.extend(face("Guard, then guard counter"));
    s.extend([
        Hold(Btn::Guard, true),
        Until(Cond::Blocked),
        Do(calm),
        Hold(Btn::Guard, false),
        Attack(Btn::Heavy),
        free,
        Wait(40),
    ]);
    s.extend(face("Knocked down: roll to get up"));
    s.extend([
        Until(Cond::Hurt),
        Do(calm),
        Until(Cond::Frame(38.0)),
        Stick(BACK, Gait::Run),
        Tap(Btn::Dodge),
        stop,
        free,
        Wait(40),
    ]);
    for caption in ["Hit reaction: stagger", "Hit reaction: flinch", "Hit reaction: large stagger"] {
        s.extend(face(caption));
        s.extend([Until(Cond::Hurt), Do(calm), free, Wait(40)]);
    }
    s.extend([Do(refresh), Press(Key::Lock), Wait(30)]);

    // --- Stairs and falls -------------------------------------------------------
    s.extend([
        Say("Stairs"),
        Go(6.5, 11.5, Gait::Run),
        Go(16.0, 11.5, Gait::Run),
        Say("A short fall"),
        Go(16.0, 14.5, Gait::Run),
        free,
        Wait(30),
        Say("Sprinting up the stairs"),
        Go(6.5, 14.5, Gait::Sprint),
        Go(6.5, 11.5, Gait::Sprint),
        Go(42.0, 11.5, Gait::Sprint),
        Say("A long fall hurts"),
        Go(42.0, 15.0, Gait::Sprint),
        free,
        Wait(60),
        Say("All the way to the top"),
        Go(6.5, 14.5, Gait::Sprint),
        Go(6.5, 11.5, Gait::Sprint),
        Go(52.5, 11.5, Gait::Sprint),
        Say("Too far"),
        Go(58.0, 11.5, Gait::Run),
        Until(Cond::Dead),
        Wait(240),
        Say(""),
        Wait(60),
    ]);
    s
}

// --- Running it in the app ------------------------------------------------------

#[derive(Resource, Default)]
pub struct Demo {
    director: Option<Director>,
    pub caption: &'static str,
}

impl Demo {
    pub fn running(&self) -> bool {
        self.director.is_some()
    }

    fn stop(&mut self, cam: &mut CamRig, options: &mut Options, pending: &mut Pending) {
        self.director = None;
        self.caption = "";
        cam.demo = None;
        options.show_help = true;
        *pending = Pending::default();
    }
}

/// Enter starts the tour from a fresh arena, or stops it.
pub fn control(
    keys: Res<ButtonInput<KeyCode>>,
    mut demo: ResMut<Demo>,
    mut sim: ResMut<Sim>,
    mut pending: ResMut<Pending>,
    mut cam: ResMut<CamRig>,
    mut options: ResMut<Options>,
) {
    if !keys.just_pressed(KeyCode::Enter) && !keys.just_pressed(KeyCode::NumpadEnter) {
        return;
    }
    if demo.running() {
        demo.stop(&mut cam, &mut options, &mut pending);
        return;
    }
    sim.0 = World::new(Level::arena());
    *pending = Pending::default();
    *cam = CamRig::default();
    options.show_help = false;
    demo.director = Some(Director::new());
}

/// Replaces the player's input with the tour's while it runs.
pub fn drive(
    mut demo: ResMut<Demo>,
    mut sim: ResMut<Sim>,
    mut pending: ResMut<Pending>,
    mut cam: ResMut<CamRig>,
    mut options: ResMut<Options>,
) {
    let demo = &mut *demo;
    let Some(director) = &mut demo.director else {
        return;
    };
    match director.drive(&mut sim.0, cam.yaw) {
        Some(input) => {
            pending.0 = input;
            cam.demo = Some(director.cam);
            demo.caption = director.caption;
        }
        None => demo.stop(&mut cam, &mut options, &mut pending),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::angle_diff;

    /// Plays the whole tour without a window and checks it shows what it says.
    #[test]
    fn the_tour_shows_everything() {
        let mut world = World::new(Level::arena());
        let mut director = Director::new();
        let mut cam = 0.0_f32;
        let mut seen: Vec<String> = Vec::new();
        let mut log: Vec<String> = Vec::new();
        let mut trace = Vec::new();
        let mut caption = "";
        let mut last = String::new();
        let mut weapons = vec![world.player.weapon];
        let mut grips = vec![world.player.grip];
        let mut top = 0.0_f32;
        let mut ticks = 0;
        loop {
            cam = match world.target() {
                Some(target) => yaw_of(target - world.player.pos),
                None => cam + angle_diff(cam, director.cam) * 0.06,
            };
            let Some(input) = director.drive(&mut world, cam) else {
                break;
            };
            world.step(&input);
            ticks += 1;
            assert!(ticks < 60 * 60 * 8, "the tour never ends; stuck on {caption:?}");

            if director.caption != caption {
                caption = director.caption;
                trace.push(format!("{ticks:6} == {caption}"));
            }
            let p = &world.player;
            let name = match p.state {
                State::Act(a) => format!("{:?}", a.id),
                State::Air(_) => "Air".to_string(),
                State::Dead { .. } => "Dead".to_string(),
                State::Ground if p.crouching => "Crouch".to_string(),
                State::Ground if p.sprinting => "Sprint".to_string(),
                State::Ground => String::new(),
            };
            let name = if p.air_attack.is_some() { format!("AirAttack+{name}") } else { name };
            if name != last && !name.is_empty() {
                trace.push(format!("{ticks:6}    {name}  at ({:.1}, {:.1}, {:.1})", p.pos.x, p.pos.y, p.pos.z));
                seen.push(name.clone());
            }
            last = name;
            for line in world.log.drain(..) {
                trace.push(format!("{ticks:6}    > {line}"));
                log.push(line);
            }
            if !weapons.contains(&p.weapon) {
                weapons.push(p.weapon);
            }
            if !grips.contains(&p.grip) {
                grips.push(p.grip);
            }
            top = top.max(p.pos.y);
        }
        println!("{}", trace.join("\n"));
        println!("{ticks} ticks = {:.0} s", ticks as f32 / 60.0);

        let has = |what: &str| seen.iter().any(|name| name.contains(what));
        for what in [
            "Sprint",
            "SprintStop",
            "Crouch",
            "CrouchRoll",
            "Roll(Light, Front)",
            "Roll(Medium, Front)",
            "Roll(Heavy, Front)",
            "Backstep",
            "Jump(Stand)",
            "Jump(Run)",
            "Jump(Sprint)",
            "LandRun",
            "LandSprint",
            "LandFall",
            "LandHeavy",
            "Roll(Medium, Left)",
            "Roll(Medium, Right)",
            "Roll(Medium, Back)",
            "Jump(RunLeft)",
            "Jump(RunRight)",
            "Jump(RunBack)",
            "LandStrafe(Left)",
            "LandStrafe(Right)",
            "LandStrafe(Back)",
            "Light1)",
            "Light2)",
            "Light3)",
            "Heavy1)",
            "Heavy1Charge)",
            "Heavy2",
            "RunLight)",
            "RunHeavy)",
            "RollAttack)",
            "BackstepAttack)",
            "CrouchAttack)",
            "AirAttack",
            "JumpLightLand",
            "JumpHeavyLand",
            "GuardCounter)",
            "LeftLight1)",
            "LeftLight2)",
            "PairedLight1)",
            "PairedLight4)",
            "PairedRun)",
            "PairedRoll)",
            "PairedJumpLand",
            "GuardHit",
            "Hurt(Small",
            "Hurt(Middle",
            "Hurt(Large",
            "Hurt(Knockdown",
            "Dead",
        ] {
            assert!(has(what), "the tour never showed {what}");
        }
        for what in ["Dodged (i-frames)", "Jumped over the sweep", "Blocked - guard counter ready"] {
            assert!(log.iter().any(|line| line == what), "the tour never got: {what}");
        }
        assert_eq!(weapons.len(), SHIELD, "every weapon class is shown");
        assert_eq!(grips.len(), 3, "every grip is shown");
        assert_eq!(world.player.left, SHIELD);
        assert!(top > 20.0, "it reaches the top of the stairs");
        assert_eq!(world.player.weapon, DEFAULT_WEAPON);
    }
}
