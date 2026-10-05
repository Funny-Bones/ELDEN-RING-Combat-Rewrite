//! The arena and the sparring dummy.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::sim::dummy::{DState, STRIKE, WINDUP};
use crate::Sim;

const DUMMY_IDLE: Color = Color::srgb(0.5, 0.42, 0.3);
const DUMMY_HOSTILE: Color = Color::srgb(0.55, 0.3, 0.25);
/// Telegraph colours: an overhead slam to roll or block, a sweep to jump.
const DUMMY_SLAM: Color = Color::srgb(1.0, 0.35, 0.1);
const DUMMY_SWEEP: Color = Color::srgb(0.2, 0.5, 1.0);
const DUMMY_STAGGER: Color = Color::srgb(0.9, 0.9, 0.9);
const DUMMY_DEAD: Color = Color::srgb(0.2, 0.2, 0.2);

#[derive(Resource)]
pub struct DummyView {
    root: Entity,
    arm: Entity,
    material: Handle<StandardMaterial>,
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
) {
    commands.spawn((
        DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: true, ..default() },
        Transform::default().looking_to(Vec3::new(-0.5, -1.0, 0.35), Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 350.0, ..default() });

    let ground = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.23, 0.2),
        perceptual_roughness: 0.95,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(400.0, 400.0))),
        MeshMaterial3d(ground),
    ));

    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.42, 0.41, 0.38),
        perceptual_roughness: 0.9,
        ..default()
    });
    for block in &sim.0.level.blocks {
        let size = block.max - block.min;
        let centre = (block.min + block.max) / 2.0;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(size.x, block.top, size.y))),
            MeshMaterial3d(stone.clone()),
            Transform::from_xyz(centre.x, block.top / 2.0, centre.y),
        ));
    }

    let material = materials.add(StandardMaterial {
        base_color: DUMMY_IDLE,
        perceptual_roughness: 0.8,
        ..default()
    });
    let root = commands
        .spawn((Transform::from_translation(sim.0.dummy.pos), Visibility::default()))
        .id();
    let mut part = |commands: &mut Commands, parent: Entity, mesh: Mesh, at: Vec3| {
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at),
            ChildOf(parent),
        ));
    };
    part(&mut commands, root, Capsule3d::new(0.5, 1.3).into(), Vec3::Y * 1.15);
    part(&mut commands, root, Sphere::new(0.3).into(), Vec3::Y * 2.45);
    part(&mut commands, root, Cuboid::new(0.3, 0.08, 0.2).into(), Vec3::new(0.0, 2.48, 0.25));
    let arm = commands
        .spawn((Transform::from_xyz(-0.65, 1.7, 0.0), Visibility::default(), ChildOf(root)))
        .id();
    part(&mut commands, arm, Capsule3d::new(0.13, 2.2).into(), Vec3::Y * -1.2);

    commands.insert_resource(DummyView { root, arm, material });
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn animate_dummy(
    view: Res<DummyView>,
    sim: Res<Sim>,
    mut transforms: Query<&mut Transform>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // Carries the sweep's spin through the recovery so it finishes the turn.
    mut sweeping: Local<bool>,
) {
    let dummy = &sim.0.dummy;
    let t = dummy.t;

    // Arm angles in degrees: `raise` swings the club forward and up over the
    // head, `out` lifts it sideways for the sweep.
    let (raise, out, spin, lean, colour) = match dummy.state {
        DState::Idle => {
            *sweeping = false;
            (8.0, 0.0, 0.0, 0.0, if dummy.aggressive { DUMMY_HOSTILE } else { DUMMY_IDLE })
        }
        DState::Windup { low: false } => (8.0 + 200.0 * ease(t / WINDUP), 0.0, 0.0, -8.0, DUMMY_SLAM),
        DState::Windup { low: true } => (20.0, 115.0 * ease(t / WINDUP), -60.0 * ease(t / WINDUP), 0.0, DUMMY_SWEEP),
        DState::Strike { low: false } => (208.0 - 135.0 * (t / STRIKE).min(1.0), 0.0, 0.0, 14.0, DUMMY_SLAM),
        DState::Strike { low: true } => {
            *sweeping = true;
            (20.0, 115.0, -60.0 + 200.0 * (t / STRIKE).min(1.0), 0.0, DUMMY_SWEEP)
        }
        DState::Recover if *sweeping => {
            let k = ease(t / 0.35);
            (20.0, 115.0 * (1.0 - ease(t / 0.9)), 140.0 + 220.0 * k, 0.0, DUMMY_HOSTILE)
        }
        DState::Recover => (73.0 - 65.0 * ease(t / 1.0), 0.0, 0.0, 14.0 * (1.0 - ease(t / 0.8)), DUMMY_HOSTILE),
        DState::Stagger => (8.0, 30.0, 0.0, -18.0 * (1.0 - ease((t - 1.0) / 0.6)), DUMMY_STAGGER),
        DState::Dead => (8.0, 0.0, 0.0, -90.0 * ease(t / 0.6), DUMMY_DEAD),
    };

    if let Ok(mut transform) = transforms.get_mut(view.root) {
        transform.rotation = Quat::from_rotation_y(dummy.yaw + f32::to_radians(spin) % TAU)
            * Quat::from_rotation_x(f32::to_radians(lean));
    }
    if let Ok(mut transform) = transforms.get_mut(view.arm) {
        // The arm hangs down -Y on the dummy's right (-X) side.
        transform.rotation = Quat::from_rotation_z(-f32::to_radians(out))
            * Quat::from_rotation_x(-f32::to_radians(raise));
    }
    if materials.get(&view.material).is_some_and(|m| m.base_color != colour) {
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.base_color = colour;
        }
    }
}

/// With no textures, a grid is what makes speed and distance readable.
pub fn draw_grid(mut gizmos: Gizmos) {
    gizmos.grid(
        Isometry3d::new(Vec3::Y * 0.01, Quat::from_rotation_x(FRAC_PI_2)),
        UVec2::splat(100),
        Vec2::splat(2.0),
        Color::srgba(0.6, 0.65, 0.6, 0.18),
    );
}
