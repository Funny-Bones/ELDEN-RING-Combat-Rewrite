//! Elden Ring-style player movement and combat on a primitive rig.
//!
//! `sim` is the whole game: a pure 60 Hz state machine with no engine types.
//! Everything else here only feeds it input and draws what it says.

use bevy::prelude::*;

mod anim;
mod camera;
mod hud;
mod input;
mod rig;
mod sim;
mod view;

use sim::level::Level;
use sim::player::Input as SimInput;

#[derive(Resource)]
pub struct Sim(pub sim::World);

/// Input accumulated since the last simulation tick. Button edges are OR-ed in
/// so a tap shorter than a tick is never lost.
#[derive(Resource, Default)]
pub struct Pending(pub SimInput);

/// Where the player was one tick ago, and where they are drawn this frame.
#[derive(Resource, Default)]
pub struct Rendered {
    prev_pos: Vec3,
    prev_yaw: f32,
    pub pos: Vec3,
    pub yaw: f32,
    /// How far this frame sits between the last two simulation ticks, 0..1.
    pub alpha: f32,
}

#[derive(Resource)]
pub struct Options {
    /// Tint the character while invincible.
    pub show_iframes: bool,
    pub show_help: bool,
}

fn main() {
    let clips = match anim::Clips::load() {
        Ok(clips) => clips,
        Err(error) => {
            eprintln!(
                "Could not load {}: {error}
Run it from the project folder, and bake the animations first with:
    python tools/bake_anims.py",
                anim::PATH
            );
            std::process::exit(1);
        }
    };
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Tarnished - movement & combat sandbox".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(Time::<Fixed>::from_hz(sim::data::TICK_HZ))
        .insert_resource(ClearColor(Color::srgb(0.07, 0.08, 0.1)))
        .insert_resource(clips)
        .insert_resource(Sim(sim::World::new(Level::arena())))
        .insert_resource(Options { show_iframes: true, show_help: true })
        .init_resource::<Pending>()
        .init_resource::<Rendered>()
        .init_resource::<camera::CamRig>()
        .add_systems(Startup, (view::setup, rig::setup, hud::setup, camera::setup))
        .add_systems(
            RunFixedMainLoop,
            (input::grab_cursor, camera::look, input::gather)
                .chain()
                .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
        )
        .add_systems(FixedUpdate, tick)
        .add_systems(
            Update,
            (
                interpolate,
                input::debug_keys,
                rig::animate,
                view::animate_dummy,
                view::draw_grid,
                camera::follow,
                hud::update,
            )
                .chain(),
        )
        .run();
}

fn tick(
    mut sim: ResMut<Sim>,
    mut pending: ResMut<Pending>,
    mut rendered: ResMut<Rendered>,
    mut cam: ResMut<camera::CamRig>,
) {
    rendered.prev_pos = sim.0.player.pos;
    rendered.prev_yaw = sim.0.player.yaw;
    sim.0.step(&pending.0);
    if sim.0.recenter_camera {
        cam.recenter_on(sim.0.player.yaw);
    }

    // Edges have been consumed; held state carries over.
    let inp = &mut pending.0;
    for button in [&mut inp.dodge, &mut inp.jump, &mut inp.light, &mut inp.heavy, &mut inp.guard] {
        button.pressed = false;
        button.released = false;
    }
    inp.crouch = false;
    inp.lock = false;
    inp.two_hand_right = false;
    inp.two_hand_left = false;
    inp.next_weapon = false;
}

fn interpolate(sim: Res<Sim>, time: Res<Time<Fixed>>, mut rendered: ResMut<Rendered>) {
    let alpha = time.overstep_fraction();
    let player = &sim.0.player;
    // A respawn is a teleport, not something to glide across.
    let alpha = if rendered.prev_pos.distance(player.pos) > 3.0 { 1.0 } else { alpha };
    rendered.alpha = alpha;
    rendered.pos = rendered.prev_pos.lerp(player.pos, alpha);
    rendered.yaw = rendered.prev_yaw + sim::angle_diff(rendered.prev_yaw, player.yaw) * alpha;
}
