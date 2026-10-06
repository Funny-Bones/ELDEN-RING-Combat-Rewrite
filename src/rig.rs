//! The player rig: primitive shapes strung along the game's own skeleton and
//! driven by its real animations. The simulation says which animation is
//! playing and on which frame; this only looks that pose up and draws it.

use bevy::prelude::*;

use crate::anim::{vec3, Clips};
use crate::sim::data::*;
use crate::sim::player::{Player, State};
use crate::{Options, Rendered, Sim};

const ARMOUR: Color = Color::srgb(0.36, 0.38, 0.43);
const INVINCIBLE: Color = Color::srgb(0.3, 0.75, 1.0);

// Animation ids within a category. Stances (how a weapon is carried) each have
// their own idle and guard; two-handed stances also have their own locomotion.
const IDLE: u32 = 0;
const GUARD: u32 = 100;
const WALK: u32 = 20000;
const RUN: u32 = 20100;
const SPRINT: u32 = 20200;
const CROUCH_IDLE: u32 = 300000;
const CROUCH_WALK: u32 = 320000;
const CROUCH_RUN: u32 = 320100;
/// In the air with nothing else going on: after a jump's arc has played out,
/// and for the body of any fall. The game's plain fall-loop entries play
/// this same clip.
const AIR_LOOP: &str = "a000_202040";
/// Stepping off a ledge, before settling into the loop.
const FALL_START: &str = "a000_004000";
const FALL_START_FRAMES: f32 = 24.0;

/// Tallest step the feet are fitted to; anything bigger is a ledge.
const FOOT_REACH: f32 = 0.5;
/// How fast the body and feet settle onto new ground heights, per second.
const BODY_SETTLE: f32 = 16.0;
const FOOT_SETTLE: f32 = 24.0;
const DEATH: &str = "a000_017002";

fn clip_name(category: u8, id: u32) -> String {
    format!("a{category:03}_{id:06}")
}

/// Stance category for how the player is currently holding things.
fn stance(p: &Player) -> u8 {
    match p.grip {
        Grip::OneHand => WEAPONS[p.weapon].stance[0],
        Grip::TwoHandRight => WEAPONS[p.weapon].stance[1],
        Grip::TwoHandLeft => WEAPONS[SHIELD].stance[1],
    }
}

/// Seconds for an arm layer (guard, two-handed carry) to blend in or out.
const LAYER_BLEND: f32 = 0.12;
/// Joints an arm layer takes over, carried on the current torso.
const LEFT_ARM: [&str; 5] = ["L_Clavicle", "L_UpperArm", "L_Forearm", "L_Hand", "L_Weapon"];
const RIGHT_ARM: [&str; 5] = ["R_Clavicle", "R_UpperArm", "R_Forearm", "R_Hand", "R_Weapon"];

#[derive(Clone, Copy)]
enum Stuff {
    Steel,
    Wood,
}

/// Stand-in weapon models, in `WEAPONS` order: boxes as (size, centre) in the
/// weapon bone's frame, where +Y runs from the grip toward the tip.
const WEAPON_PARTS: &[&[([f32; 3], [f32; 3], Stuff)]] = &[
    // Dagger
    &[([0.03, 0.13, 0.03], [0.0, -0.02, 0.0], Stuff::Wood), ([0.1, 0.02, 0.03], [0.0, 0.05, 0.0], Stuff::Steel), ([0.04, 0.33, 0.012], [0.0, 0.22, 0.0], Stuff::Steel)],
    // Longsword
    &[([0.03, 0.2, 0.03], [0.0, -0.02, 0.0], Stuff::Wood), ([0.2, 0.025, 0.04], [0.0, 0.09, 0.0], Stuff::Steel), ([0.05, 0.86, 0.012], [0.0, 0.53, 0.0], Stuff::Steel)],
    // Claymore
    &[([0.035, 0.34, 0.035], [0.0, -0.06, 0.0], Stuff::Wood), ([0.32, 0.03, 0.045], [0.0, 0.12, 0.0], Stuff::Steel), ([0.075, 1.25, 0.014], [0.0, 0.75, 0.0], Stuff::Steel)],
    // Greatsword
    &[([0.045, 0.42, 0.045], [0.0, -0.08, 0.0], Stuff::Wood), ([0.38, 0.045, 0.06], [0.0, 0.14, 0.0], Stuff::Steel), ([0.17, 1.65, 0.03], [0.0, 0.98, 0.0], Stuff::Steel)],
    // Rapier
    &[([0.025, 0.16, 0.025], [0.0, -0.02, 0.0], Stuff::Wood), ([0.11, 0.05, 0.11], [0.0, 0.08, 0.0], Stuff::Steel), ([0.018, 0.98, 0.018], [0.0, 0.59, 0.0], Stuff::Steel)],
    // Uchigatana
    &[([0.03, 0.26, 0.03], [0.0, -0.04, 0.0], Stuff::Wood), ([0.08, 0.015, 0.08], [0.0, 0.1, 0.0], Stuff::Steel), ([0.034, 0.92, 0.01], [0.0, 0.57, 0.0], Stuff::Steel)],
    // Club
    &[([0.04, 0.4, 0.04], [0.0, 0.08, 0.0], Stuff::Wood), ([0.1, 0.3, 0.1], [0.0, 0.42, 0.0], Stuff::Wood)],
    // Battle Axe
    &[([0.035, 0.8, 0.035], [0.0, 0.28, 0.0], Stuff::Wood), ([0.24, 0.2, 0.02], [0.1, 0.58, 0.0], Stuff::Steel)],
    // Short Spear
    &[([0.03, 1.8, 0.03], [0.0, 0.5, 0.0], Stuff::Wood), ([0.05, 0.28, 0.014], [0.0, 1.52, 0.0], Stuff::Steel)],
    // Halberd
    &[([0.035, 2.0, 0.035], [0.0, 0.55, 0.0], Stuff::Wood), ([0.26, 0.24, 0.02], [0.11, 1.42, 0.0], Stuff::Steel), ([0.04, 0.3, 0.014], [0.0, 1.7, 0.0], Stuff::Steel)],
    // Heavy thrusting sword
    &[([0.03, 0.22, 0.03], [0.0, -0.03, 0.0], Stuff::Wood), ([0.13, 0.06, 0.13], [0.0, 0.1, 0.0], Stuff::Steel), ([0.028, 1.2, 0.028], [0.0, 0.72, 0.0], Stuff::Steel)],
    // Curved sword: two offset lengths suggest the curve
    &[([0.03, 0.18, 0.03], [0.0, -0.02, 0.0], Stuff::Wood), ([0.14, 0.02, 0.04], [0.0, 0.08, 0.0], Stuff::Steel), ([0.055, 0.45, 0.012], [0.0, 0.32, 0.0], Stuff::Steel), ([0.06, 0.4, 0.012], [0.035, 0.72, 0.0], Stuff::Steel)],
    // Curved greatsword
    &[([0.04, 0.34, 0.04], [0.0, -0.06, 0.0], Stuff::Wood), ([0.22, 0.03, 0.05], [0.0, 0.12, 0.0], Stuff::Steel), ([0.1, 0.7, 0.016], [0.0, 0.5, 0.0], Stuff::Steel), ([0.11, 0.62, 0.016], [0.06, 1.12, 0.0], Stuff::Steel)],
    // Twinblade: a blade off each end of the grip
    &[([0.035, 0.5, 0.035], [0.0, 0.0, 0.0], Stuff::Wood), ([0.05, 0.8, 0.012], [0.0, 0.65, 0.0], Stuff::Steel), ([0.05, 0.8, 0.012], [0.0, -0.65, 0.0], Stuff::Steel)],
    // Great hammer
    &[([0.045, 1.15, 0.045], [0.0, 0.4, 0.0], Stuff::Wood), ([0.32, 0.22, 0.22], [0.0, 1.05, 0.0], Stuff::Steel)],
    // Flail: handle, chain, head
    &[([0.035, 0.42, 0.035], [0.0, 0.08, 0.0], Stuff::Wood), ([0.015, 0.3, 0.015], [0.0, 0.44, 0.0], Stuff::Steel), ([0.15, 0.15, 0.15], [0.0, 0.66, 0.0], Stuff::Steel)],
    // Greataxe
    &[([0.045, 1.25, 0.045], [0.0, 0.45, 0.0], Stuff::Wood), ([0.42, 0.36, 0.03], [0.17, 0.98, 0.0], Stuff::Steel)],
    // Great spear
    &[([0.04, 2.3, 0.04], [0.0, 0.7, 0.0], Stuff::Wood), ([0.08, 0.42, 0.02], [0.0, 2.05, 0.0], Stuff::Steel)],
    // Reaper
    &[([0.035, 1.8, 0.035], [0.0, 0.55, 0.0], Stuff::Wood), ([0.62, 0.09, 0.015], [0.3, 1.42, 0.0], Stuff::Steel)],
    // Whip
    &[([0.035, 0.2, 0.035], [0.0, -0.02, 0.0], Stuff::Wood), ([0.02, 2.0, 0.02], [0.0, 1.08, 0.0], Stuff::Wood)],
    // Fist
    &[([0.1, 0.12, 0.1], [0.0, 0.03, 0.0], Stuff::Steel)],
    // Claw
    &[([0.09, 0.08, 0.05], [0.0, 0.0, 0.0], Stuff::Wood), ([0.012, 0.3, 0.012], [-0.03, 0.19, 0.0], Stuff::Steel), ([0.012, 0.3, 0.012], [0.0, 0.19, 0.0], Stuff::Steel), ([0.012, 0.3, 0.012], [0.03, 0.19, 0.0], Stuff::Steel)],
    // Colossal weapon
    &[([0.06, 1.1, 0.06], [0.0, 0.35, 0.0], Stuff::Wood), ([0.36, 0.62, 0.36], [0.0, 1.25, 0.0], Stuff::Steel)],
    // Torch
    &[([0.04, 0.5, 0.04], [0.0, 0.15, 0.0], Stuff::Wood), ([0.09, 0.13, 0.09], [0.0, 0.46, 0.0], Stuff::Steel)],
];

#[derive(Clone, Copy)]
enum Skin {
    Armour,
    Cloth,
}

/// (from joint, to joint, radius, material)
const SEGMENTS: &[(&str, &str, f32, Skin)] = &[
    ("Pelvis", "Spine1", 0.125, Skin::Cloth),
    ("Spine1", "Spine2", 0.135, Skin::Cloth),
    ("Spine2", "Neck", 0.12, Skin::Cloth),
    ("Neck", "Head", 0.05, Skin::Armour),
    ("L_Thigh", "R_Thigh", 0.095, Skin::Armour),
    ("L_UpperArm", "R_UpperArm", 0.07, Skin::Armour),
    ("L_UpperArm", "L_Forearm", 0.05, Skin::Armour),
    ("L_Forearm", "L_Hand", 0.042, Skin::Armour),
    ("R_UpperArm", "R_Forearm", 0.05, Skin::Armour),
    ("R_Forearm", "R_Hand", 0.042, Skin::Armour),
    ("L_Thigh", "L_Calf", 0.075, Skin::Armour),
    ("L_Calf", "L_Foot", 0.058, Skin::Armour),
    ("L_Foot", "L_Toe0", 0.045, Skin::Armour),
    ("R_Thigh", "R_Calf", 0.075, Skin::Armour),
    ("R_Calf", "R_Foot", 0.058, Skin::Armour),
    ("R_Foot", "R_Toe0", 0.045, Skin::Armour),
];

/// (joint, radius): rounds off the ends of the segments.
const BALLS: &[(&str, f32)] = &[
    ("L_UpperArm", 0.07),
    ("R_UpperArm", 0.07),
    ("L_Forearm", 0.05),
    ("R_Forearm", 0.05),
    ("L_Hand", 0.05),
    ("R_Hand", 0.05),
    ("L_Thigh", 0.095),
    ("R_Thigh", 0.095),
    ("L_Calf", 0.075),
    ("R_Calf", 0.075),
    ("L_Foot", 0.058),
    ("R_Foot", 0.058),
    ("L_Toe0", 0.045),
    ("R_Toe0", 0.045),
    ("Pelvis", 0.125),
    ("Neck", 0.12),
];

#[derive(Resource)]
pub struct Rig {
    root: Entity,
    segments: Vec<(Entity, usize, usize)>,
    balls: Vec<(Entity, usize)>,
    head: Entity,
    /// Pivot on the right-hand weapon bone, and the models that can sit on it.
    sword: Entity,
    weapons: Vec<Entity>,
    right_shield: Entity,
    /// Pivot on the left-hand weapon bone, holding the shield.
    shield: Entity,
    armour: Handle<StandardMaterial>,
    /// Clip currently playing (and the frame and whether it loops, for its
    /// sounds), and the pose being cross-faded out of.
    pub clip: String,
    pub frame: f32,
    pub looped: bool,
    from: Vec<f32>,
    fade: f32,
    fade_len: f32,
    shown: Vec<f32>,
    /// Stride phase, 0..1, shared by every locomotion loop so that changing
    /// gait or direction keeps the feet in step.
    phase: f32,
    guard: f32,
    carry: f32,
    /// Smoothed height the body stands at, so steps are climbed, not snapped up.
    body_y: f32,
    /// How far each foot (left, right) is lifted to meet the ground under it.
    foot_lift: [f32; 2],
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    clips: Res<Clips>,
) {
    let armour = materials.add(StandardMaterial {
        base_color: ARMOUR,
        perceptual_roughness: 0.6,
        metallic: 0.4,
        ..default()
    });
    let cloth = materials.add(Color::srgb(0.42, 0.16, 0.13));
    let steel = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.87, 0.9),
        perceptual_roughness: 0.25,
        metallic: 0.9,
        ..default()
    });
    let wood = materials.add(Color::srgb(0.4, 0.27, 0.15));

    let root = commands.spawn((Transform::default(), Visibility::default())).id();
    let mut part = |commands: &mut Commands,
                    parent: Entity,
                    mesh: Mesh,
                    material: &Handle<StandardMaterial>,
                    at: Vec3| {
        commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(at),
                ChildOf(parent),
            ))
            .id()
    };
    let c = &mut commands;

    // Unit-height cylinders, stretched between their two joints every frame.
    let segments = SEGMENTS
        .iter()
        .map(|&(a, b, radius, skin)| {
            let material = match skin {
                Skin::Armour => &armour,
                Skin::Cloth => &cloth,
            };
            let entity = part(c, root, Cylinder::new(radius, 1.0).into(), material, Vec3::ZERO);
            (entity, clips.joint(a), clips.joint(b))
        })
        .collect();
    let balls = BALLS
        .iter()
        .map(|&(joint, radius)| {
            let material = if matches!(joint, "Pelvis" | "Neck") { &cloth } else { &armour };
            (part(c, root, Sphere::new(radius).into(), material, Vec3::ZERO), clips.joint(joint))
        })
        .collect();

    let pivot = |c: &mut Commands| c.spawn((Transform::default(), Visibility::default(), ChildOf(root))).id();

    // Local frame: +Y up through the skull, +Z out of the face.
    let head = pivot(c);
    part(c, head, Sphere::new(0.115).into(), &armour, Vec3::Y * 0.09);
    part(c, head, Cuboid::new(0.13, 0.035, 0.07).into(), &steel, Vec3::new(0.0, 0.1, 0.09));

    // One hidden model per weapon on the right-hand bone; `animate` shows the equipped one.
    let sword = pivot(c);
    let group = |c: &mut Commands, parent: Entity| {
        c.spawn((Transform::default(), Visibility::Hidden, ChildOf(parent))).id()
    };
    let mut weapons = Vec::new();
    for parts in WEAPON_PARTS {
        let model = group(c, sword);
        for &(size, at, stuff) in *parts {
            let material = match stuff {
                Stuff::Steel => &steel,
                Stuff::Wood => &wood,
            };
            part(c, model, Cuboid::from_size(Vec3::from(size)).into(), material, Vec3::from(at));
        }
        weapons.push(model);
    }

    // The shield lives on the left-hand bone, or on the right one while it is two-handed.
    let shield = pivot(c);
    let right_shield = group(c, sword);
    for parent in [shield, right_shield] {
        part(c, parent, Cuboid::new(0.56, 0.42, 0.045).into(), &wood, Vec3::ZERO);
        part(c, parent, Cuboid::new(0.14, 0.14, 0.06).into(), &steel, Vec3::ZERO);
    }

    commands.insert_resource(Rig {
        root,
        segments,
        balls,
        head,
        sword,
        weapons,
        right_shield,
        shield,
        armour,
        clip: String::new(),
        frame: 0.0,
        looped: false,
        from: Vec::new(),
        fade: 0.0,
        fade_len: 0.0,
        shown: Vec::new(),
        phase: 0.0,
        guard: 0.0,
        carry: 0.0,
        body_y: 0.0,
        foot_lift: [0.0; 2],
    });
}

/// Which animation the simulation's state corresponds to: (clip, frame, loops).
fn playing(p: &Player, rig: &mut Rig, clips: &Clips, time: f32, dt: f32, ahead: f32) -> (String, f32, bool) {
    if let Some(attack) = p.air_attack {
        return (attack.def.source.to_string(), attack.f + ahead, false);
    }
    let stance = stance(p);
    match p.state {
        State::Dead { t } => (DEATH.to_string(), t + ahead, false),
        // Only a walk off a ledge has a fall start; a jump is already airborne.
        State::Air(a) if !a.jumped && a.f < FALL_START_FRAMES => (FALL_START.to_string(), a.f + ahead, false),
        State::Air(_) => (AIR_LOOP.to_string(), time * ANIM_FPS, true),
        State::Act(a) => (a.id.def().source.to_string(), a.f + ahead, false),
        State::Ground => {
            if p.speed < 0.05 {
                return if p.crouching {
                    (clip_name(0, CROUCH_IDLE), time * ANIM_FPS, true)
                } else {
                    (clip_name(stance, IDLE), time * ANIM_FPS, true)
                };
            }
            let facing = p.facing();
            let along = p.move_dir.dot(facing);
            let across = p.move_dir.dot(Vec3::new(facing.z, 0.0, -facing.x));
            // Forward, back, left, right.
            let dir = if along.abs() >= across.abs() {
                (along < 0.0) as u32
            } else {
                2 + (across < 0.0) as u32
            };
            let (id, native) = if p.crouching && p.speed > (CROUCH_WALK_SPEED + CROUCH_RUN_SPEED) / 2.0 {
                (CROUCH_RUN + dir, CROUCH_RUN_SPEED)
            } else if p.crouching {
                (CROUCH_WALK + dir, CROUCH_WALK_SPEED)
            } else if p.sprinting && p.speed > RUN_SPEED {
                (SPRINT, SPRINT_SPEED)
            } else if p.speed > (WALK_SPEED + RUN_SPEED) / 2.0 {
                (RUN + dir, [RUN_SPEED, RUN_BACK_SPEED, RUN_SIDE_SPEED, RUN_SIDE_SPEED][dir as usize])
            } else {
                (WALK + dir, WALK_SPEED)
            };
            // Two-handed stances bring their own loops; everything else moves on the base set.
            let mut clip = clip_name(stance, id);
            if p.grip == Grip::OneHand || p.crouching || clips.get(&clip).is_none() {
                clip = clip_name(0, id);
            }
            // Play the loop at whatever rate keeps the feet matched to the ground speed.
            let last = clips.get(&clip).map_or(1.0, |c| (c.frames - 1) as f32);
            rig.phase = (rig.phase + p.speed / native * ANIM_FPS * dt / last).fract();
            (clip, rig.phase * last, true)
        }
    }
}

/// Rotation taking local X and Y onto the given axes. The baked axes are
/// mirrored, so the third is rebuilt to keep the basis right-handed.
fn basis(x: Vec3, y: Vec3) -> Quat {
    let x = x.normalize_or(Vec3::X);
    let z = x.cross(y).normalize_or(Vec3::Z);
    Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z))
}

pub fn animate(
    mut rig: ResMut<Rig>,
    clips: Res<Clips>,
    sim: Res<Sim>,
    rendered: Res<Rendered>,
    options: Res<Options>,
    time: Res<Time>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut target: Local<Vec<f32>>,
    mut layer: Local<Vec<f32>>,
) {
    let player = &sim.0.player;
    let dt = time.delta_secs();
    let rig = &mut *rig;

    let (name, frame, looped) = playing(player, rig, &clips, time.elapsed_secs(), dt, rendered.alpha * DF);
    let Some(clip) = clips.get(&name) else {
        return;
    };
    clips.sample(clip, frame, looped, &mut target);

    rig.frame = frame;
    rig.looped = looped;
    // Cross-fade from whatever was on screen, over the clip's authored blend time.
    if name != rig.clip {
        rig.clip = name;
        rig.from.clone_from(&rig.shown);
        rig.fade = 0.0;
        rig.fade_len = if rig.from.is_empty() { 0.0 } else { clip.blend / ANIM_FPS };
    }
    rig.fade += dt;
    let t = if rig.fade_len > 0.0 { (rig.fade / rig.fade_len).clamp(0.0, 1.0) } else { 1.0 };
    let t = t * t * (3.0 - 2.0 * t);
    rig.shown.clear();
    if t >= 1.0 {
        rig.shown.extend_from_slice(&target);
    } else {
        rig.shown.extend(rig.from.iter().zip(target.iter()).map(|(a, b)| a + (b - a) * t));
    }

    let two_handed = player.grip != Grip::OneHand;
    let stance = stance(player);
    let grounded = matches!(player.state, State::Ground);
    let raising = player.guarding || matches!(player.state, State::Act(a) if a.id == ActionId::GuardHit);
    let step = dt / LAYER_BLEND;
    rig.guard = (rig.guard + if raising { step } else { -step }).clamp(0.0, 1.0);
    // One-handed stances have no locomotion of their own: the base loops play
    // with the stance's weapon arm layered over them.
    let carrying = !two_handed && stance != 0 && grounded && player.speed >= 0.05;
    rig.carry = (rig.carry + if carrying { step } else { -step }).clamp(0.0, 1.0);

    let mut pose = std::mem::take(&mut *target);
    pose.clone_from(&rig.shown);
    // An arm layer takes those joints from another clip, re-seated on the
    // chest of whatever the body is doing.
    let mut overlay = |pose: &mut Vec<f32>, name: &str, arms: &[&[&str]], weight: f32| {
        let Some(clip) = clips.get(&name) else {
            return;
        };
        if weight <= 0.0 {
            return;
        }
        clips.sample(clip, time.elapsed_secs() * ANIM_FPS, true, &mut layer);
        let chest = clips.joint("Spine2");
        let offset = vec3(pose, chest) - vec3(&layer, chest);
        for joint in arms.iter().flat_map(|arm| arm.iter()) {
            let at = clips.joint(joint);
            let blended = vec3(pose, at).lerp(vec3(&layer, at) + offset, weight);
            pose[at..at + 3].copy_from_slice(&blended.to_array());
            if joint.ends_with("Weapon") {
                let at = clips.axes(joint);
                for i in at..at + 9 {
                    pose[i] += (layer[i] - pose[i]) * weight;
                }
            }
        }
    };
    overlay(&mut pose, &clip_name(stance, IDLE), &[&RIGHT_ARM], rig.carry);
    // One-handed, the guard is the shield arm alone, from the shield's own stance.
    let (guard_stance, guard_arms): (u8, &[&[&str]]) = if two_handed {
        (stance, &[&LEFT_ARM, &RIGHT_ARM])
    } else {
        (WEAPONS[SHIELD].stance[0], &[&LEFT_ARM])
    };
    overlay(&mut pose, &clip_name(guard_stance, GUARD), guard_arms, rig.guard);

    // Show whatever is actually in each hand.
    let mut show = |entity: Entity, on: bool| {
        if let Ok(mut state) = visibility.get_mut(entity) {
            let wanted = if on { Visibility::Inherited } else { Visibility::Hidden };
            if *state != wanted {
                *state = wanted;
            }
        }
    };
    for (index, &model) in rig.weapons.iter().enumerate() {
        show(model, index == player.weapon && player.grip != Grip::TwoHandLeft);
    }
    show(rig.right_shield, player.grip == Grip::TwoHandLeft);
    show(rig.shield, player.grip == Grip::OneHand);

    // --- Foot placement ---------------------------------------------------
    // On stairs the simulation snaps the character up or down a whole step.
    // Here the body eases onto the new height instead, and each foot is put
    // on whatever ground is actually under it, with the knee re-bent to suit.
    let root_rotation = Quat::from_rotation_y(rendered.yaw);
    let planted = !player.airborne() && !matches!(player.state, State::Air(_) | State::Dead { .. });
    if !planted || (rendered.pos.y - rig.body_y).abs() > FOOT_REACH * 2.0 {
        rig.body_y = rendered.pos.y;
    } else {
        rig.body_y += (rendered.pos.y - rig.body_y) * (1.0 - (-BODY_SETTLE * dt).exp());
    }
    let level = &sim.0.level;
    let legs = [["L_Thigh", "L_Calf", "L_Foot", "L_Toe0"], ["R_Thigh", "R_Calf", "R_Foot", "R_Toe0"]];
    for (side, leg) in legs.iter().enumerate() {
        let foot = root_rotation * vec3(&pose, clips.joint(leg[2]));
        let ground = level.height(rendered.pos.x + foot.x, rendered.pos.z + foot.z) - rig.body_y;
        let wanted = if planted && ground.abs() <= FOOT_REACH { ground } else { 0.0 };
        rig.foot_lift[side] += (wanted - rig.foot_lift[side]) * (1.0 - (-FOOT_SETTLE * dt).exp());
    }
    // Sink the whole body far enough for the lower foot to reach its ground...
    let sink = rig.foot_lift[0].min(rig.foot_lift[1]).min(0.0);
    if sink < 0.0 {
        for y in pose[..clips.axes("Head")].iter_mut().skip(1).step_by(3) {
            *y += sink;
        }
    }
    // ...then lift each foot the rest of the way and solve its knee.
    for (side, leg) in legs.iter().enumerate() {
        let lift = rig.foot_lift[side] - sink;
        if lift.abs() < 1e-4 {
            continue;
        }
        let [hip, knee, foot, toe] = leg.map(|joint| clips.joint(joint));
        let (h, k, f) = (vec3(&pose, hip), vec3(&pose, knee), vec3(&pose, foot));
        let (thigh, shin) = ((k - h).length(), (f - k).length());
        let target = f + Vec3::Y * lift;
        let to_target = target - h;
        let reach = to_target.length().clamp((thigh - shin).abs() + 1e-3, thigh + shin - 1e-3);
        let along = to_target.normalize_or(Vec3::NEG_Y);
        // Keep the knee bending the way the animation had it: sideways of
        // the animated hip-to-foot line. A nearly straight leg has almost no
        // bend to read, so it is biased forward, where knees go; without that
        // the direction is noise and the thigh can swing out to the side.
        let axis = (f - h).normalize_or(Vec3::NEG_Y);
        let animated = (k - h) - axis * (k - h).dot(axis);
        let hint = animated + Vec3::Z * 0.08;
        let bend = (hint - along * hint.dot(along)).normalize_or(Vec3::Z);
        let x = (thigh * thigh - shin * shin + reach * reach) / (2.0 * reach);
        let new_knee = h + along * x + bend * (thigh * thigh - x * x).max(0.0).sqrt();
        let new_foot = h + along * reach;
        let new_toe = vec3(&pose, toe) + (new_foot - f);
        pose[knee..knee + 3].copy_from_slice(&new_knee.to_array());
        pose[foot..foot + 3].copy_from_slice(&new_foot.to_array());
        pose[toe..toe + 3].copy_from_slice(&new_toe.to_array());
    }

    for &(entity, a, b) in &rig.segments {
        let (a, b) = (vec3(&pose, a), vec3(&pose, b));
        if let Ok(mut transform) = transforms.get_mut(entity) {
            let span = b - a;
            let length = span.length().max(1e-4);
            transform.translation = (a + b) / 2.0;
            transform.rotation = Quat::from_rotation_arc(Vec3::Y, span / length);
            transform.scale = Vec3::new(1.0, length, 1.0);
        }
    }
    for &(entity, joint) in &rig.balls {
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation = vec3(&pose, joint);
        }
    }

    let mut place = |entity: Entity, joint: &str, rotation: Quat| {
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation = vec3(&pose, clips.joint(joint));
            transform.rotation = rotation;
        }
    };
    // The head bone's X axis runs up the neck and its Z axis out of the face.
    let at = clips.axes("Head");
    let (up, face) = (vec3(&pose, at), vec3(&pose, at + 6));
    place(rig.head, "Head", basis(up.cross(face), up));
    // Weapon bones point the blade along their Y axis.
    let at = clips.axes("R_Weapon");
    place(rig.sword, "R_Weapon", basis(vec3(&pose, at), vec3(&pose, at + 3)));
    let at = clips.axes("L_Weapon");
    place(rig.shield, "L_Weapon", basis(vec3(&pose, at), vec3(&pose, at + 3)));
    *target = pose;

    if let Ok(mut transform) = transforms.get_mut(rig.root) {
        transform.translation = Vec3::new(rendered.pos.x, rig.body_y, rendered.pos.z);
        transform.rotation = root_rotation;
    }

    let tint = if options.show_iframes && player.invincible() && !player.is_dead() {
        INVINCIBLE
    } else {
        ARMOUR
    };
    // Only touch the asset when the colour actually flips, so it is not re-uploaded every frame.
    if materials.get(&rig.armour).is_some_and(|m| m.base_color != tint) {
        if let Some(mut material) = materials.get_mut(&rig.armour) {
            material.base_color = tint;
        }
    }
}
