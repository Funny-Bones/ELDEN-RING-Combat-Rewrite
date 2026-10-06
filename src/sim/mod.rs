//! The gameplay simulation, independent of rendering. `World::step` advances
//! everything by one 60 Hz tick.

use std::f32::consts::{PI, TAU};

use bevy::math::Vec3;

pub mod data;
pub mod dummy;
pub mod extracted;
pub mod level;
pub mod player;
#[cfg(test)]
mod tests;

use data::*;
use dummy::Dummy;
use level::Level;
use player::{HitResult, Input, Player};

/// Yaw 0 faces +Z; positive yaw turns toward +X.
pub fn dir_of(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

pub fn yaw_of(dir: Vec3) -> f32 {
    dir.x.atan2(dir.z)
}

pub fn angle_diff(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}

pub fn turn_toward(yaw: f32, goal: f32, max_step: f32) -> f32 {
    yaw + angle_diff(yaw, goal).clamp(-max_step, max_step)
}

/// Shortest distance between the segments a-b and c-d.
pub fn segment_distance(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> f32 {
    let (u, v, w) = (b - a, d - c, a - c);
    let (uu, uv, vv, uw, vw) = (u.dot(u), u.dot(v), v.dot(v), u.dot(w), v.dot(w));
    let denom = uu * vv - uv * uv;
    // Closest point on a-b to the line through c-d, then on c-d to that, then back.
    let mut s = if denom > 1e-8 { ((uv * vw - vv * uw) / denom).clamp(0.0, 1.0) } else { 0.0 };
    let t = if vv > 1e-8 { ((uv * s + vw) / vv).clamp(0.0, 1.0) } else { 0.0 };
    if uu > 1e-8 {
        s = ((uv * t - uw) / uu).clamp(0.0, 1.0);
    }
    (a + u * s).distance(c + v * t)
}

pub fn approach(value: f32, goal: f32, max_step: f32) -> f32 {
    value + (goal - value).clamp(-max_step, max_step)
}

pub struct World {
    pub level: Level,
    pub player: Player,
    pub dummy: Dummy,
    pub locked: bool,
    /// Set for one tick when lock-on was pressed with nothing to lock onto,
    /// which snaps the camera behind the character instead.
    pub recenter_camera: bool,
    /// Human-readable combat events, newest last. The HUD drains this.
    pub log: Vec<String>,
}

impl World {
    pub fn new(level: Level) -> Self {
        Self {
            level,
            player: Player::new(Vec3::ZERO),
            dummy: Dummy::new(Vec3::new(0.0, 0.0, 8.0)),
            locked: false,
            recenter_camera: false,
            log: Vec::new(),
        }
    }

    pub fn target(&self) -> Option<Vec3> {
        self.locked.then_some(self.dummy.pos)
    }

    pub fn step(&mut self, inp: &Input) {
        let distance = self.player.pos.distance(self.dummy.pos);

        self.recenter_camera = false;
        if inp.lock {
            if self.locked {
                self.locked = false;
            } else if self.dummy.alive() && distance <= LOCK_ON_RANGE {
                self.locked = true;
            } else {
                self.recenter_camera = true;
            }
        }
        if self.locked && (!self.dummy.alive() || distance > LOCK_BREAK_RANGE || self.player.is_dead())
        {
            self.locked = false;
        }

        let in_combat = self.dummy.hostile() && distance < 25.0;
        self.player.step(inp, &self.level, self.target(), in_combat);
        self.separate_bodies();
        self.player_attacks();

        if let Some(hit) = self.dummy.step(self.player.pos, self.player.is_dead()) {
            // The swing stays live for its whole active window, so an evade
            // only counts if it outlasts every one of these checks.
            let result = self.player.receive_hit(&hit, &self.level);
            let message = match result {
                HitResult::Ignored => None,
                HitResult::Dodged => Some("Dodged (i-frames)".to_string()),
                HitResult::Jumped => Some("Jumped over the sweep".to_string()),
                HitResult::Blocked => Some("Blocked - guard counter ready".to_string()),
                HitResult::GuardBroken => Some("Guard broken".to_string()),
                HitResult::Hit => Some(format!("Took {:.0} damage", hit.damage)),
                HitResult::Killed => Some("YOU DIED".to_string()),
            };
            let evaded = matches!(result, HitResult::Dodged | HitResult::Jumped);
            if !evaded {
                self.dummy.connected = true;
            }
            if let Some(message) = message {
                if !(evaded && self.dummy.evade_logged) {
                    self.log.push(message);
                }
            }
            self.dummy.evade_logged |= evaded;
        }
        if self.player.is_dead() {
            self.dummy.calm();
        }
    }

    /// Characters are solid: you cannot roll through an enemy.
    fn separate_bodies(&mut self) {
        if !self.dummy.alive() {
            return;
        }
        let p = &mut self.player;
        if (p.pos.y - self.dummy.pos.y).abs() > 1.8 {
            return;
        }
        let mut away = Vec3::new(p.pos.x - self.dummy.pos.x, 0.0, p.pos.z - self.dummy.pos.z);
        let min = dummy::RADIUS + 0.35;
        let dist = away.length();
        if dist >= min {
            return;
        }
        if dist < 1e-3 {
            away = -p.facing();
        }
        let push = away.normalize() * (min - dist);
        self.level.slide(&mut p.pos, push);
    }

    fn player_attacks(&mut self) {
        let Some(hit) = self.player.active_hit() else {
            return;
        };
        if !self.dummy.alive() {
            return;
        }
        // The blade has to actually reach the dummy: its body or its head.
        let at = self.dummy.pos;
        let (feet, shoulders) = (at + Vec3::Y * dummy::RADIUS, at + Vec3::Y * (dummy::HEIGHT - dummy::RADIUS));
        let head = at + Vec3::Y * dummy::HEAD_HEIGHT;
        let touches = |&(a, b): &(Vec3, Vec3)| {
            segment_distance(a, b, feet, shoulders) <= hit.radius + dummy::RADIUS
                || segment_distance(a, b, head, head) <= hit.radius + dummy::HEAD_RADIUS
        };
        if !hit.sweep.iter().any(touches) {
            return;
        }
        let damage = hit.attack * hit.mv;
        // The dummy's poise is our own invention; scale it off the attack's weight.
        let outcome = self.dummy.take_hit(damage, hit.guard_damage * 5.0);
        self.player.mark_hit();
        // The blow lands: both freeze for a moment before carrying on.
        self.player.hit_stop = hit.stop;
        self.dummy.hit_stop = hit.stop;
        self.log.push(format!("Hit for {damage:.0}{outcome}"));
    }
}
