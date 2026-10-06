//! A stationary sparring partner. Passive until told otherwise; when hostile it
//! alternates a telegraphed overhead slam with a low sweep, which is enough to
//! exercise i-frames, guarding, guard counters and jumping over attacks.

use bevy::math::Vec3;

use super::data::{HurtLevel, DT};
use super::player::Incoming;
use super::{dir_of, turn_toward, yaw_of};

pub const RADIUS: f32 = 0.5;
pub const MAX_HP: f32 = 1200.0;
const MAX_POISE: f32 = 60.0;
const AGGRO_RANGE: f32 = 4.2;
const COOLDOWN: f32 = 1.6;
pub const WINDUP: f32 = 0.9;
/// The dummy stops tracking this long before the swing, so a late roll works.
const TRACK_UNTIL: f32 = 0.6;
pub const STRIKE: f32 = 0.1;
const RECOVER: f32 = 1.2;
const STAGGER: f32 = 1.6;
const RESPAWN: f32 = 4.0;
/// The attack cycle: (is a low sweep, how hard it hits).
const SWINGS: [(bool, HurtLevel); 4] = [
    (false, HurtLevel::Middle),
    (true, HurtLevel::Small),
    (false, HurtLevel::Large),
    (false, HurtLevel::Knockdown),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DState {
    Idle,
    Windup { low: bool },
    Strike { low: bool },
    Recover,
    Stagger,
    Dead,
}

#[derive(Clone, Debug)]
pub struct Dummy {
    pub pos: Vec3,
    pub yaw: f32,
    pub hp: f32,
    pub poise: f32,
    pub state: DState,
    /// Seconds spent in the current state.
    pub t: f32,
    pub aggressive: bool,
    /// The current swing already landed or was blocked.
    pub connected: bool,
    /// The current swing was already reported as evaded.
    pub evade_logged: bool,
    /// How hard the swing in progress knocks the player about.
    pub level: HurtLevel,
    next: u8,
    since_hit: f32,
}

impl Dummy {
    pub fn new(pos: Vec3) -> Self {
        Self {
            pos,
            yaw: std::f32::consts::PI,
            hp: MAX_HP,
            poise: MAX_POISE,
            state: DState::Idle,
            t: 0.0,
            aggressive: false,
            connected: false,
            evade_logged: false,
            level: HurtLevel::Middle,
            next: 0,
            since_hit: 0.0,
        }
    }

    pub fn alive(&self) -> bool {
        self.state != DState::Dead
    }

    pub fn hostile(&self) -> bool {
        self.aggressive && self.alive()
    }

    fn enter(&mut self, state: DState) {
        self.state = state;
        self.t = 0.0;
    }

    /// Drops whatever it was doing, e.g. when the player dies.
    pub fn calm(&mut self) {
        if matches!(self.state, DState::Windup { .. } | DState::Strike { .. } | DState::Recover) {
            self.enter(DState::Idle);
        }
    }

    /// Returns a suffix describing what the hit did, for the combat log.
    pub fn take_hit(&mut self, damage: f32, poise: f32) -> &'static str {
        self.hp -= damage;
        self.since_hit = 0.0;
        if self.hp <= 0.0 {
            self.hp = 0.0;
            self.enter(DState::Dead);
            return " - defeated";
        }
        self.poise -= poise;
        if self.poise <= 0.0 {
            self.poise = MAX_POISE;
            self.enter(DState::Stagger);
            return " - poise broken";
        }
        ""
    }

    pub fn step(&mut self, player: Vec3, player_dead: bool) -> Option<Incoming> {
        self.t += DT;
        self.since_hit += DT;
        if self.since_hit > 5.0 {
            self.poise = MAX_POISE;
        }

        let to = Vec3::new(player.x - self.pos.x, 0.0, player.z - self.pos.z);
        let distance = to.length();
        let track = |d: &mut Dummy, rate: f32| {
            if distance > 0.01 {
                d.yaw = turn_toward(d.yaw, yaw_of(to), rate.to_radians() * DT);
            }
        };

        match self.state {
            DState::Idle => {
                track(self, 180.0);
                let level = (player.y - self.pos.y).abs() < 1.5;
                if self.aggressive && !player_dead && level && self.t >= COOLDOWN && distance <= AGGRO_RANGE {
                    self.connected = false;
                    self.evade_logged = false;
                    // Slam, sweep, a harder slam, then one that knocks you down.
                    let (low, level) = SWINGS[self.next as usize % SWINGS.len()];
                    self.next = self.next.wrapping_add(1);
                    self.level = level;
                    self.enter(DState::Windup { low });
                }
            }
            DState::Windup { low } => {
                if self.t < TRACK_UNTIL {
                    track(self, 240.0);
                }
                if self.t >= WINDUP {
                    self.enter(DState::Strike { low });
                }
            }
            DState::Strike { low } => {
                if self.t >= STRIKE {
                    self.enter(DState::Recover);
                } else if !self.connected {
                    let (range, half_arc) = if low { (3.6, 180.0_f32) } else { (3.4, 40.0) };
                    let (damage, stamina) = match self.level {
                        HurtLevel::Small => (120.0, 30.0),
                        HurtLevel::Middle => (160.0, 40.0),
                        HurtLevel::Large => (200.0, 50.0),
                        HurtLevel::Knockdown => (240.0, 60.0),
                    };
                    let in_arc = to.normalize_or_zero().dot(dir_of(self.yaw)) >= half_arc.to_radians().cos();
                    let in_height = (player.y - self.pos.y).abs() < 2.5;
                    if distance <= range && in_arc && in_height {
                        return Some(Incoming { damage, stamina, from: self.pos, low, level: self.level });
                    }
                }
            }
            DState::Recover => {
                if self.t >= RECOVER {
                    self.enter(DState::Idle);
                }
            }
            DState::Stagger => {
                if self.t >= STAGGER {
                    self.enter(DState::Idle);
                }
            }
            DState::Dead => {
                if self.t >= RESPAWN {
                    let aggressive = self.aggressive;
                    *self = Dummy::new(self.pos);
                    self.aggressive = aggressive;
                }
            }
        }
        None
    }
}
