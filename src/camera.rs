//! Third-person orbit camera with lock-on.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::sim::{angle_diff, dir_of, yaw_of};
use crate::{Rendered, Sim};

const DISTANCE: f32 = 4.3;
const PIVOT_HEIGHT: f32 = 1.5;
const MOUSE_SENSITIVITY: Vec2 = Vec2::new(0.0028, 0.0022);
const STICK_SPEED: Vec2 = Vec2::new(2.8, 1.8);
const PITCH_MIN: f32 = -1.2;
const PITCH_MAX: f32 = 0.7;
const LOCKED_PITCH: f32 = -0.24;

#[derive(Resource)]
pub struct CamRig {
    /// Direction the camera looks, same convention as character yaw.
    pub yaw: f32,
    /// Negative looks down.
    pub pitch: f32,
    pivot: Vec3,
    /// Yaw the camera is swinging round to after a lock-on press with no target.
    recenter: Option<f32>,
    /// Seconds since the player last moved the camera themselves.
    idle: f32,
}

impl Default for CamRig {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: -0.25, pivot: Vec3::Y * PIVOT_HEIGHT, recenter: None, idle: 0.0 }
    }
}

impl CamRig {
    /// Swing round behind the character.
    pub fn recenter_on(&mut self, yaw: f32) {
        self.recenter = Some(yaw);
    }
}

pub fn setup(mut commands: Commands) {
    commands.spawn((Camera3d::default(), Transform::from_xyz(0.0, 3.0, -5.0)));
}

/// Runs before the simulation so movement is relative to this frame's view.
pub fn look(
    mut cam: ResMut<CamRig>,
    sim: Res<Sim>,
    mouse: Res<AccumulatedMouseMotion>,
    gamepads: Query<&Gamepad>,
    cursor: Single<&CursorOptions>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let world = &sim.0;
    let player = &world.player;

    let mut delta = Vec2::ZERO;
    if cursor.grab_mode != CursorGrabMode::None {
        delta += mouse.delta * MOUSE_SENSITIVITY;
    }
    for pad in &gamepads {
        let stick = pad.right_stick();
        if stick.length() > 0.15 {
            delta += Vec2::new(stick.x, -stick.y) * STICK_SPEED * dt;
        }
    }
    cam.idle = if delta == Vec2::ZERO { cam.idle + dt } else { 0.0 };

    if let Some(target) = world.target() {
        // Locked on: the camera sits behind the character on the line to the target.
        cam.recenter = None;
        let to = target - player.pos;
        if to.length_squared() > 0.01 {
            let ease = 1.0 - (-8.0 * dt).exp();
            cam.yaw += angle_diff(cam.yaw, yaw_of(to)) * ease;
            cam.pitch += (LOCKED_PITCH - cam.pitch) * ease;
        }
        return;
    }

    if delta != Vec2::ZERO {
        cam.recenter = None;
        cam.yaw -= delta.x;
        cam.pitch = (cam.pitch - delta.y).clamp(PITCH_MIN, PITCH_MAX);
    } else if let Some(goal) = cam.recenter {
        let ease = 1.0 - (-12.0 * dt).exp();
        cam.yaw += angle_diff(cam.yaw, goal) * ease;
        cam.pitch += (-0.25 - cam.pitch) * ease;
        if angle_diff(cam.yaw, goal).abs() < 0.01 {
            cam.recenter = None;
        }
    } else if cam.idle > 1.0 && player.speed > 1.0 {
        // Left alone, the camera drifts round to follow sideways movement.
        // Running straight at or away from it leaves it be.
        let off = angle_diff(cam.yaw, yaw_of(player.move_dir));
        cam.yaw += off.sin() * 0.6 * dt;
    }
}

pub fn follow(
    mut cam: ResMut<CamRig>,
    rendered: Res<Rendered>,
    sim: Res<Sim>,
    time: Res<Time>,
    mut transform: Single<&mut Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs();
    let goal = rendered.pos + Vec3::Y * PIVOT_HEIGHT;
    if cam.pivot.distance(goal) > 6.0 {
        cam.pivot = goal;
    }
    // Horizontal follow is tight; vertical is lazier so jumps and stairs do not jolt the view.
    let flat = 1.0 - (-14.0 * dt).exp();
    let lift = 1.0 - (-6.0 * dt).exp();
    cam.pivot.x += (goal.x - cam.pivot.x) * flat;
    cam.pivot.z += (goal.z - cam.pivot.z) * flat;
    cam.pivot.y += (goal.y - cam.pivot.y) * lift;

    let look = dir_of(cam.yaw) * cam.pitch.cos() + Vec3::Y * cam.pitch.sin();
    // Pull the camera in front of whatever geometry sits between it and the character.
    let level = &sim.0.level;
    let step = DISTANCE / 32.0;
    let blocked = (1..=32).map(|i| step * i as f32).find(|&d| {
        let point = cam.pivot - look * d;
        level.height(point.x, point.z) > point.y - 0.2
    });
    let distance = blocked.map_or(DISTANCE, |d| (d - step).max(0.5));
    let eye = cam.pivot - look * distance;
    **transform = Transform::from_translation(eye).looking_at(cam.pivot, Vec3::Y);
}
