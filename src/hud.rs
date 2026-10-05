//! Bars, lock-on marker and a debug readout of the state machine.

use bevy::prelude::*;

use crate::sim::data::*;
use crate::sim::dummy;
use crate::sim::player::State;
use crate::{Options, Sim};

const BAR_BACK: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
const LOG_LINES: usize = 6;

#[derive(Component)]
pub enum Bar {
    Hp,
    Stamina,
    Dummy,
}

#[derive(Component)]
pub enum Label {
    Debug,
    Log,
    Help,
    Died,
}

#[derive(Component)]
pub struct LockMarker;

const HELP: &str = "\
WASD move   Mouse look   Alt walk
Space tap: roll / backstep (on release)
Space hold: sprint        F jump   X crouch
LMB light   Shift+LMB heavy (hold to charge)
RMB guard   then Shift+LMB: guard counter
E+LMB two-hand weapon   E+RMB two-hand shield
Right arrow: next weapon
Q / MMB lock-on
1/2/3 light/medium/heavy load
T dummy hostile   F1 i-frame tint   H help
Pad: B dodge/sprint  A jump  RB/RT attack
     LB guard  L3 crouch  R3 lock-on
     Y+RB / Y+LB two-hand  D-pad right: weapon";

pub fn setup(mut commands: Commands) {
    let font = |size: f32| TextFont { font_size: FontSize::Px(size), ..default() };

    let bar = |commands: &mut Commands, parent: Entity, kind: Bar, width: f32, colour: Color| {
        let back = commands
            .spawn((
                Node { width: px(width), height: px(11), ..default() },
                BackgroundColor(BAR_BACK),
                ChildOf(parent),
            ))
            .id();
        commands.spawn((
            Node { width: percent(100), height: percent(100), ..default() },
            BackgroundColor(colour),
            kind,
            ChildOf(back),
        ));
    };

    let bars = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: px(28),
            top: px(24),
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            ..default()
        })
        .id();
    bar(&mut commands, bars, Bar::Hp, MAX_HP * 0.6, Color::srgb(0.62, 0.12, 0.1));
    bar(&mut commands, bars, Bar::Stamina, MAX_STAMINA * 2.4, Color::srgb(0.25, 0.55, 0.22));

    let boss = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: percent(25),
            bottom: px(48),
            width: percent(50),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        })
        .id();
    commands.spawn((Text::new("Sparring Dummy"), font(18.0), ChildOf(boss)));
    let back = commands
        .spawn((
            Node { width: percent(100), height: px(9), ..default() },
            BackgroundColor(BAR_BACK),
            ChildOf(boss),
        ))
        .id();
    commands.spawn((
        Node { width: percent(100), height: percent(100), ..default() },
        BackgroundColor(Color::srgb(0.62, 0.12, 0.1)),
        Bar::Dummy,
        ChildOf(back),
    ));

    commands.spawn((
        Text::new(""),
        font(15.0),
        Label::Debug,
        Node { position_type: PositionType::Absolute, right: px(20), top: px(20), ..default() },
    ));
    commands.spawn((
        Text::new(""),
        font(15.0),
        TextColor(Color::srgb(0.95, 0.85, 0.5)),
        Label::Log,
        Node { position_type: PositionType::Absolute, left: px(28), top: px(70), ..default() },
    ));
    commands.spawn((
        Text::new(HELP),
        font(13.0),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
        Label::Help,
        Node { position_type: PositionType::Absolute, left: px(28), bottom: px(20), ..default() },
    ));
    commands.spawn((
        Text::new("YOU DIED"),
        font(72.0),
        TextColor(Color::srgb(0.6, 0.08, 0.06)),
        Label::Died,
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            top: percent(40),
            justify_content: JustifyContent::Center,
            ..default()
        },
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: px(10),
            height: px(10),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(Color::WHITE),
        Visibility::Hidden,
        LockMarker,
    ));
}

pub fn update(
    mut sim: ResMut<Sim>,
    options: Res<Options>,
    mut bars: Query<(&mut Node, &Bar), Without<LockMarker>>,
    mut labels: Query<(&mut Text, &mut Visibility, &Label), Without<LockMarker>>,
    mut marker: Single<(&mut Node, &mut Visibility), With<LockMarker>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut log: Local<Vec<String>>,
) {
    // Bypass change detection for the read-mostly world; only the log is drained.
    let world = &mut sim.bypass_change_detection().0;
    log.append(&mut world.log);
    let overflow = log.len().saturating_sub(LOG_LINES);
    log.drain(..overflow);
    let player = &world.player;

    for (mut node, bar) in &mut bars {
        let fill = match bar {
            Bar::Hp => player.hp / MAX_HP,
            Bar::Stamina => player.stamina / MAX_STAMINA,
            Bar::Dummy => world.dummy.hp / dummy::MAX_HP,
        };
        node.width = percent(fill.clamp(0.0, 1.0) * 100.0);
    }

    for (mut text, mut visibility, label) in &mut labels {
        match label {
            Label::Debug => text.0 = debug_text(world),
            Label::Log => text.0 = log.join("\n"),
            Label::Help => {
                *visibility = if options.show_help { Visibility::Inherited } else { Visibility::Hidden }
            }
            Label::Died => {
                *visibility = if player.is_dead() { Visibility::Inherited } else { Visibility::Hidden }
            }
        }
    }

    let (node, visibility) = &mut *marker;
    let (cam, cam_transform) = *camera;
    let spot = world
        .target()
        .and_then(|target| cam.world_to_viewport(cam_transform, target + Vec3::Y * 1.4).ok());
    match spot {
        Some(spot) => {
            node.left = px(spot.x - 5.0);
            node.top = px(spot.y - 5.0);
            **visibility = Visibility::Inherited;
        }
        None => **visibility = Visibility::Hidden,
    }
}

fn debug_text(world: &crate::sim::World) -> String {
    let p = &world.player;
    let state = match p.state {
        State::Ground => {
            let gait = if p.speed < 0.05 {
                "Idle"
            } else if p.sprinting {
                "Sprint"
            } else if p.speed <= WALK_SPEED + 0.1 {
                "Walk"
            } else {
                "Run"
            };
            let stance = match (p.crouching, p.guarding) {
                (true, true) => " (crouch, guard)",
                (true, false) => " (crouch)",
                (false, true) => " (guard)",
                (false, false) => "",
            };
            format!("{gait}{stance}")
        }
        State::Act(a) => {
            let def = a.id.def();
            let charge = match def.charge {
                Some((from, to)) if a.f >= from && a.f < to => "  charging",
                Some((_, to)) if a.f >= to => "  CHARGED",
                _ => "",
            };
            let attack = match p.air_attack {
                Some(attack) if attack.heavy => "  + heavy attack",
                Some(_) => "  + light attack",
                None => "",
            };
            format!("{}  f{:>4.1}/{:.0}{charge}{attack}\n[{}]", def.name, a.f, def.total, def.source)
        }
        State::Air(a) => match p.air_attack {
            Some(attack) if attack.heavy => "Air (heavy attack)".to_string(),
            Some(_) => "Air (light attack)".to_string(),
            None if !a.falling() => "Jump (airborne)".to_string(),
            None => "Fall".to_string(),
        },
        State::Dead { .. } => "Dead".to_string(),
    };
    let flag = |on: bool, text: &'static str| if on { text } else { "" };
    format!(
        "{state}\n\
         speed {:.1} m/s   height {:.1} m\n\
         {}\n\
         load: {:?}   {}\n\
         {}{}{}{}\n\
         queued: {}",
        p.speed,
        p.pos.y,
        match p.grip {
            Grip::OneHand => format!("{} + Shield", WEAPONS[p.weapon].name),
            Grip::TwoHandRight => format!("{} (two-handed)", WEAPONS[p.weapon].name),
            Grip::TwoHandLeft => "Shield (two-handed)".to_string(),
        },
        p.load,
        if p.in_combat { "IN COMBAT (sprint drains)" } else { "out of combat (sprint is free)" },
        flag(p.invincible() && !p.is_dead(), "[I-FRAMES] "),
        flag(p.active_hit().is_some(), "[HIT ACTIVE] "),
        flag(p.guard_counter_ready(), "[GUARD COUNTER] "),
        flag(world.locked, "[LOCKED ON] "),
        p.buffer.map_or("-".to_string(), |req| format!("{req:?}")),
    )
}
