//! Behaviour tests for the state machine. Expected numbers are the ones
//! extracted from the game (see `extracted.rs`), so these also guard against
//! the simulation drifting away from the data it is fed.

use bevy::math::{Vec2, Vec3};

use super::data::*;
use super::level::Level;
use super::player::{Button, Input, State};
use super::World;

const HELD: Button = Button { held: true, pressed: false, released: false };
const DOWN: Button = Button { held: true, pressed: true, released: false };
const UP: Button = Button { held: false, pressed: false, released: true };
const MEDIUM_ROLL: ActionId = ActionId::Roll(Load::Medium, Dir::Front);

/// An attack in the default moveset: the Longsword, one-handed.
fn atk(kind: AttackKind) -> ActionId {
    ActionId::Attack(Moveset { weapon: DEFAULT_WEAPON as u8, two_hand: false }, kind)
}

fn world() -> World {
    World::new(Level::flat())
}

fn forward() -> Input {
    Input { mv: Vec2::Y, ..Default::default() }
}

fn idle() -> Input {
    Input::default()
}

fn run(w: &mut World, inp: Input, ticks: usize) {
    for _ in 0..ticks {
        w.step(&inp);
    }
}

fn action(w: &World) -> Option<(ActionId, f32)> {
    match w.player.state {
        State::Act(a) => Some((a.id, a.f)),
        _ => None,
    }
}

fn id(w: &World) -> Option<ActionId> {
    action(w).map(|(id, _)| id)
}

/// Steps with `inp` until the current action reaches `frame`.
fn run_to_frame(w: &mut World, inp: Input, frame: f32) {
    while action(w).is_some_and(|(_, f)| f < frame) {
        w.step(&inp);
    }
}

/// Press and release the dodge button over three ticks.
fn tap_dodge(w: &mut World, base: Input) {
    w.step(&Input { dodge: DOWN, ..base });
    w.step(&Input { dodge: HELD, ..base });
    assert!(action(w).is_none(), "nothing happens while the button is down");
    w.step(&Input { dodge: UP, ..base });
}

#[test]
fn roll_comes_out_on_release() {
    let mut w = world();
    tap_dodge(&mut w, forward());
    assert_eq!(id(&w), Some(MEDIUM_ROLL));
}

#[test]
fn dodge_without_direction_is_a_backstep() {
    let mut w = world();
    tap_dodge(&mut w, idle());
    assert_eq!(id(&w), Some(ActionId::Backstep));
    let mut invincible = 0;
    while action(&w).is_some() {
        invincible += w.player.invincible() as usize;
        w.step(&idle());
    }
    assert_eq!(invincible, 14, "the backstep has 7 i-frames");
    assert!((w.player.pos.z + 2.5).abs() < 1e-2, "and travels 2.5 m away from facing");
}

#[test]
fn holding_dodge_sprints_and_release_does_not_roll() {
    let mut w = world();
    w.step(&Input { dodge: DOWN, ..forward() });
    run(&mut w, Input { dodge: HELD, ..forward() }, 60);
    assert!(w.player.sprinting);
    assert!((w.player.speed - SPRINT_SPEED).abs() < 1e-3);
    w.step(&Input { dodge: UP, ..forward() });
    run(&mut w, forward(), 5);
    assert!(action(&w).is_none());
    assert!(!w.player.sprinting);
}

#[test]
fn medium_roll_has_thirteen_iframes_and_its_extracted_distance() {
    let mut w = world();
    tap_dodge(&mut w, forward());
    let start = w.player.pos;
    let mut invincible_ticks = 0;
    while let Some((_, f)) = action(&w) {
        assert_eq!(w.player.invincible(), f < 13.0);
        invincible_ticks += w.player.invincible() as usize;
        w.step(&idle());
    }
    assert_eq!(invincible_ticks, 26, "13 frames at 30 fps is 26 ticks at 60 Hz");
    assert!((w.player.pos.distance(start) - 3.65).abs() < 0.01);
}

#[test]
fn roll_distance_and_recovery_follow_equip_load() {
    let mut distances = Vec::new();
    for load in [Load::Light, Load::Medium, Load::Heavy] {
        let mut w = world();
        w.player.load = load;
        tap_dodge(&mut w, forward());
        assert_eq!(id(&w), Some(ActionId::Roll(load, Dir::Front)));
        let start = w.player.pos;
        run(&mut w, idle(), 140);
        distances.push(w.player.pos.distance(start));
    }
    assert!((distances[0] - 4.33).abs() < 0.01);
    assert!((distances[1] - 3.65).abs() < 0.01);
    assert!((distances[2] - 3.31).abs() < 0.01);
    let heavy = ActionId::Roll(Load::Heavy, Dir::Front).def();
    assert_eq!(heavy.iframes, (0.0, 12.0));
    assert!(heavy.cancel_dodge > MEDIUM_ROLL.def().cancel_dodge);
}

#[test]
fn locked_on_side_roll_keeps_facing_the_target() {
    let mut w = world();
    w.step(&Input { lock: true, ..idle() });
    assert!(w.locked);
    // Stick left of the camera, which looks down +Z: the +X side.
    let left = Input { mv: Vec2::new(-1.0, 0.0), ..idle() };
    w.step(&Input { dodge: DOWN, ..left });
    w.step(&Input { dodge: UP, ..left });
    assert_eq!(id(&w), Some(ActionId::Roll(Load::Medium, Dir::Left)));
    while matches!(id(&w), Some(ActionId::Roll(..))) {
        assert!(w.player.yaw.abs() < 1e-3, "still squared up to the target");
        w.step(&idle());
    }
    assert!((w.player.pos.x - 4.41).abs() < 0.05, "x = {}", w.player.pos.x);
    assert!(w.player.pos.z.abs() < 0.05);
}

#[test]
fn presses_before_the_input_window_are_lost() {
    let mut w = world();
    w.step(&Input { light: DOWN, ..idle() });
    assert_eq!(id(&w), Some(atk(AttackKind::Light1)));
    let def = atk(AttackKind::Light1).def();
    // Mash during the first frames, before the animation is listening.
    run_to_frame(&mut w, Input { light: DOWN, ..idle() }, def.input_from - 1.0);
    assert!(w.player.buffer.is_none());
    run(&mut w, idle(), 200);
    assert!(action(&w).is_none(), "the swing just ended; nothing was queued");
}

#[test]
fn queued_attack_fires_exactly_at_the_cancel_frame() {
    let mut w = world();
    w.step(&Input { light: DOWN, ..idle() });
    let def = atk(AttackKind::Light1).def();
    run_to_frame(&mut w, idle(), def.input_from);
    w.step(&Input { light: DOWN, ..idle() });
    assert!(w.player.buffer.is_some());
    loop {
        let (current, f) = action(&w).unwrap();
        if current != atk(AttackKind::Light1) {
            assert_eq!(current, atk(AttackKind::Light2));
            break;
        }
        assert!(f < def.cancel_light + DF);
        w.step(&idle());
    }
}

#[test]
fn light_chain_runs_five_deep_then_loops() {
    let mut w = world();
    let mut seen = Vec::new();
    for _ in 0..700 {
        w.step(&Input { light: DOWN, ..idle() });
        w.player.stamina = MAX_STAMINA;
        if let Some(current) = id(&w) {
            if seen.last() != Some(&current) {
                seen.push(current);
            }
        }
    }
    use AttackKind::*;
    assert_eq!(&seen[..6], &[Light1, Light2, Light3, Light4, Light5, Light1].map(atk));
}

#[test]
fn combo_resets_once_the_animation_is_left() {
    let mut w = world();
    w.step(&Input { light: DOWN, ..idle() });
    run(&mut w, idle(), 200);
    assert!(action(&w).is_none());
    w.step(&Input { light: DOWN, ..idle() });
    assert_eq!(id(&w), Some(atk(AttackKind::Light1)));
}

#[test]
fn attack_stamina_is_taken_when_the_hit_comes_out() {
    let mut w = world();
    w.step(&Input { light: DOWN, ..idle() });
    let hit = atk(AttackKind::Light1).def().hit.unwrap();
    run_to_frame(&mut w, idle(), hit.from - 0.5);
    assert_eq!(w.player.stamina, MAX_STAMINA);
    w.step(&idle());
    assert_eq!(w.player.stamina, MAX_STAMINA - 12.0);
}

#[test]
fn attack_after_roll_is_the_rolling_attack() {
    let mut w = world();
    tap_dodge(&mut w, forward());
    run_to_frame(&mut w, idle(), MEDIUM_ROLL.def().input_from);
    w.step(&Input { light: DOWN, ..idle() });
    run_to_frame(&mut w, idle(), MEDIUM_ROLL.def().cancel_light);
    assert_eq!(id(&w), Some(atk(AttackKind::RollAttack)));
}

#[test]
fn attack_while_sprinting_is_the_running_attack() {
    let mut w = world();
    w.step(&Input { dodge: DOWN, ..forward() });
    run(&mut w, Input { dodge: HELD, ..forward() }, 40);
    w.step(&Input { dodge: HELD, light: DOWN, ..forward() });
    assert_eq!(id(&w), Some(atk(AttackKind::RunLight)));
}

#[test]
fn crouch_attack_stands_the_character_up() {
    let mut w = world();
    w.step(&Input { crouch: true, ..idle() });
    assert!(w.player.crouching);
    w.step(&Input { light: DOWN, ..idle() });
    assert_eq!(id(&w), Some(atk(AttackKind::CrouchAttack)));
    assert!(!w.player.crouching);
}

#[test]
fn tapped_heavy_releases_and_held_heavy_charges() {
    let swing = |held: bool| {
        let mut w = world();
        w.step(&Input { heavy: DOWN, ..idle() });
        assert_eq!(id(&w), Some(atk(AttackKind::Heavy1Charge)));
        let hold = if held { HELD } else { Button::default() };
        let (mut best, mut ids) = (0.0_f32, Vec::new());
        while let Some(current) = id(&w) {
            if ids.last() != Some(&current) {
                ids.push(current);
            }
            if let Some(hit) = w.player.active_hit() {
                best = best.max(hit.mv);
            }
            w.step(&Input { heavy: hold, ..idle() });
        }
        (ids, best, MAX_STAMINA - w.player.stamina)
    };
    let (ids, mv, _) = swing(false);
    assert_eq!(ids, [atk(AttackKind::Heavy1Charge), atk(AttackKind::Heavy1)]);
    assert!((mv - 1.25).abs() < 1e-3);
    let (ids, mv, _) = swing(true);
    assert_eq!(ids, [atk(AttackKind::Heavy1Charge)]);
    assert!((mv - 1.60).abs() < 1e-3);
    assert_eq!(atk(AttackKind::Heavy1).def().stamina, 20.0);
    assert_eq!(atk(AttackKind::Heavy1Charge).def().stamina, 30.0);
}

#[test]
fn jump_follows_the_authored_arc() {
    let mut w = world();
    w.step(&Input { jump: DOWN, ..idle() });
    assert_eq!(id(&w), Some(ActionId::Jump(JumpKind::Stand)));
    let mut apex = 0.0_f32;
    let mut liftoff = None;
    for tick in 0..200 {
        w.step(&idle());
        apex = apex.max(w.player.pos.y);
        if liftoff.is_none() && w.player.pos.y > 0.0 {
            liftoff = Some(tick);
        }
    }
    assert!((apex - 1.13).abs() < 0.01, "apex {apex}");
    assert!(liftoff.unwrap() >= 12, "feet stay down for the first six frames");
    assert_eq!(w.player.pos.y, 0.0);

    let mut w = world();
    w.step(&Input { jump: DOWN, ..forward() });
    assert_eq!(id(&w), Some(ActionId::Jump(JumpKind::Run)));
    run(&mut w, idle(), 200);
    assert!(w.player.pos.z > 3.2, "a running jump clears over 3.2 m, got {}", w.player.pos.z);
}

#[test]
fn jump_attack_lands_into_its_recovery() {
    let mut w = world();
    w.step(&Input { jump: DOWN, ..idle() });
    run_to_frame(&mut w, idle(), 8.0);
    w.step(&Input { heavy: DOWN, ..idle() });
    assert!(w.player.air_attack.is_some());
    while matches!(id(&w), Some(ActionId::Jump(_))) || matches!(w.player.state, State::Air(_)) {
        w.step(&idle());
    }
    assert_eq!(id(&w), Some(atk(AttackKind::JumpHeavyLand)));
    assert!(w.player.air_attack.is_none());
    assert_eq!(MAX_STAMINA - w.player.stamina, JUMP_COST + 20.0, "charged once, not twice");
}

#[test]
fn no_jump_attack_before_the_jump_allows_it() {
    let mut w = world();
    w.step(&Input { jump: DOWN, ..idle() });
    w.step(&Input { light: DOWN, ..idle() });
    assert!(w.player.air_attack.is_none());
    run_to_frame(&mut w, idle(), 6.5);
    assert!(w.player.air_attack.is_some(), "the queued press fires once frame 6 is reached");
}

#[test]
fn sprinting_only_costs_stamina_in_combat() {
    let mut w = world();
    w.step(&Input { dodge: DOWN, ..forward() });
    run(&mut w, Input { dodge: HELD, ..forward() }, 120);
    assert_eq!(w.player.stamina, MAX_STAMINA);

    let mut w = world();
    w.dummy.aggressive = true;
    w.player.pos = Vec3::new(10.0, 0.0, 0.0);
    w.step(&Input { dodge: DOWN, mv: Vec2::new(1.0, 0.0), ..idle() });
    run(&mut w, Input { dodge: HELD, mv: Vec2::new(1.0, 0.0), ..idle() }, 60);
    assert!(w.player.stamina < MAX_STAMINA - 5.0);
}

#[test]
fn no_stamina_no_roll() {
    let mut w = world();
    w.player.stamina = 0.0;
    let tap = Button { held: false, pressed: true, released: true };
    w.step(&Input { dodge: tap, ..forward() });
    assert!(action(&w).is_none());
    assert!(w.player.buffer.is_none(), "the request is dropped, not kept for later");
}

#[test]
fn walking_off_a_tall_ledge_is_fatal() {
    let mut w = World::new(Level::arena());
    w.player.pos = Vec3::new(52.0, 21.0, 14.0);
    let mut died = false;
    for _ in 0..400 {
        w.step(&forward());
        died |= w.player.is_dead();
    }
    assert!(died);
}

#[test]
fn stairs_are_walked_not_fallen_down() {
    let mut w = World::new(Level::arena());
    w.player.pos = Vec3::new(7.0, 0.0, 11.5);
    // Camera yaw 0 looks down +Z, so stick-left is +X: up the stairs.
    let up = Input { mv: Vec2::new(-1.0, 0.0), ..idle() };
    for _ in 0..240 {
        w.step(&up);
        assert!(matches!(w.player.state, State::Ground));
    }
    assert!(w.player.pos.y > 4.0);
}

#[test]
fn block_then_heavy_is_a_guard_counter() {
    let mut w = world();
    w.dummy.aggressive = true;
    w.player.pos = Vec3::new(0.0, 0.0, 5.0);
    let guard = Input { guard: HELD, ..idle() };
    let mut blocked = false;
    for _ in 0..400 {
        w.step(&guard);
        if w.log.iter().any(|l| l.starts_with("Blocked")) {
            blocked = true;
            break;
        }
    }
    assert!(blocked, "log: {:?}", w.log);
    assert_eq!(w.player.hp, MAX_HP);
    assert!(w.player.stamina < MAX_STAMINA);
    w.step(&Input { heavy: DOWN, ..idle() });
    let mut countered = false;
    for _ in 0..40 {
        w.step(&idle());
        countered |= id(&w) == Some(atk(AttackKind::GuardCounter));
    }
    assert!(countered);
}

#[test]
fn unguarded_hit_staggers_and_roll_iframes_avoid_it() {
    let mut w = world();
    w.dummy.aggressive = true;
    w.player.pos = Vec3::new(0.0, 0.0, 5.0);
    let mut hit = false;
    for _ in 0..400 {
        w.step(&idle());
        if id(&w) == Some(ActionId::Hurt(HurtLevel::Middle, Dir::Front)) {
            hit = true;
            break;
        }
    }
    assert!(hit);
    assert!(w.player.hp < MAX_HP);

    // Same setup, but roll into the swing just before it lands.
    let mut w = world();
    w.dummy.aggressive = true;
    w.player.pos = Vec3::new(0.0, 0.0, 5.0);
    loop {
        w.step(&idle());
        if let super::dummy::DState::Windup { .. } = w.dummy.state {
            if w.dummy.t >= super::dummy::WINDUP - 4.0 * DT {
                break;
            }
        }
    }
    w.step(&Input { dodge: DOWN, ..forward() });
    w.step(&Input { dodge: UP, ..forward() });
    run(&mut w, idle(), 60);
    assert_eq!(w.player.hp, MAX_HP, "log: {:?}", w.log);
    assert!(w.log.iter().any(|l| l.starts_with("Dodged")));
}

#[test]
fn lock_on_needs_a_target_in_range() {
    let mut w = world();
    w.step(&Input { lock: true, ..idle() });
    assert!(w.locked);
    w.step(&Input { lock: true, ..idle() });
    assert!(!w.locked);

    w.player.pos = Vec3::new(0.0, 0.0, -40.0);
    w.step(&Input { lock: true, ..idle() });
    assert!(!w.locked);
    assert!(w.recenter_camera);
}

#[test]
fn player_attack_damages_the_dummy_once_per_swing() {
    let mut w = world();
    w.player.pos = Vec3::new(0.0, 0.0, 6.0);
    w.step(&Input { light: DOWN, ..idle() });
    run(&mut w, idle(), 160);
    let expected = super::dummy::MAX_HP - 110.0 * atk(AttackKind::Light1).def().hit.unwrap().mv;
    assert_eq!(WEAPONS[DEFAULT_WEAPON].attack, 110.0);
    assert!((w.dummy.hp - expected).abs() < 1e-3);
}

#[test]
fn extracted_speeds_are_the_games() {
    assert!((WALK_SPEED - 1.5).abs() < 0.01);
    assert!((RUN_SPEED - 4.012).abs() < 0.01);
    assert!((SPRINT_SPEED - 6.035).abs() < 0.01);
}

#[test]
fn two_handing_the_weapon_switches_to_its_two_handed_moveset() {
    let mut w = world();
    w.step(&Input { two_hand_right: true, ..idle() });
    assert_eq!(w.player.grip, Grip::TwoHandRight);
    w.step(&Input { light: DOWN, ..idle() });
    let two_handed = Moveset { weapon: DEFAULT_WEAPON as u8, two_hand: true };
    assert_eq!(id(&w), Some(ActionId::Attack(two_handed, AttackKind::Light1)));
    assert_eq!(id(&w).unwrap().def().source, "a023_032000");

    // The grip cannot change in the middle of a swing.
    w.step(&Input { two_hand_right: true, ..idle() });
    assert_eq!(w.player.grip, Grip::TwoHandRight);
    run(&mut w, idle(), 300);
    w.step(&Input { two_hand_right: true, ..idle() });
    assert_eq!(w.player.grip, Grip::OneHand);
}

#[test]
fn two_handing_the_left_hand_uses_the_shield_moveset() {
    let mut w = world();
    w.step(&Input { two_hand_left: true, ..idle() });
    assert_eq!(w.player.grip, Grip::TwoHandLeft);
    assert_eq!(w.player.moveset().info().name, "Shield");
    w.step(&Input { light: DOWN, ..idle() });
    assert_eq!(id(&w).unwrap().def().source, "a048_032000");
    run(&mut w, idle(), 300);
    // Going straight from one two-handed grip to the other.
    w.step(&Input { two_hand_right: true, ..idle() });
    assert_eq!(w.player.grip, Grip::TwoHandRight);
}

#[test]
fn weapon_swap_cycles_through_everything_but_the_shield() {
    let mut w = world();
    let mut seen = vec![w.player.weapon];
    for _ in 0..SHIELD {
        w.step(&Input { next_weapon: true, ..idle() });
        seen.push(w.player.weapon);
    }
    assert_eq!(seen.first(), seen.last(), "back where it started");
    assert!(!seen.contains(&SHIELD));
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), SHIELD);
}

#[test]
fn every_moveset_has_its_core_attacks() {
    use AttackKind::*;
    for weapon in 0..WEAPONS.len() {
        for two_hand in [false, true] {
            let moveset = Moveset { weapon: weapon as u8, two_hand };
            for kind in [Light1, Light2, Heavy1Charge, Heavy1, RunLight, RunHeavy, RollAttack, BackstepAttack] {
                let def = moveset.attack(kind).unwrap_or_else(|| panic!("{} {two_hand} {kind:?}", WEAPONS[weapon].name));
                let hit = def.hit.unwrap();
                assert!(hit.from < hit.to && hit.to <= def.total, "{} {kind:?}", WEAPONS[weapon].name);
                assert!(def.stamina > 0.0 && hit.mv > 0.0);
            }
        }
    }
}

#[test]
fn light_chain_length_depends_on_the_weapon() {
    let chain = |name: &str| {
        let mut w = world();
        w.player.weapon = WEAPONS.iter().position(|info| info.name == name).unwrap();
        let mut seen = Vec::new();
        for _ in 0..900 {
            w.step(&Input { light: DOWN, ..idle() });
            w.player.stamina = MAX_STAMINA;
            if let Some(ActionId::Attack(_, kind)) = id(&w) {
                if seen.last() != Some(&kind) {
                    seen.push(kind);
                }
            }
        }
        seen.iter().skip(1).position(|&kind| kind == AttackKind::Light1).unwrap() + 1
    };
    assert_eq!(chain("Longsword"), 5);
    assert_eq!(chain("Greatsword"), 3);
    assert_eq!(chain("Rapier"), 6);
}

#[test]
fn every_moveset_can_jump_attack() {
    for weapon in 0..WEAPONS.len() {
        for two_hand in [false, true] {
            let moveset = Moveset { weapon: weapon as u8, two_hand };
            for heavy in [false, true] {
                let air = moveset.air(heavy).unwrap_or_else(|| panic!("{} {two_hand} {heavy}", WEAPONS[weapon].name));
                assert!(air.from < air.to);
            }
        }
    }
}

#[test]
fn crouching_has_its_own_walk_and_run_speeds() {
    let mut w = world();
    w.step(&Input { crouch: true, ..idle() });
    run(&mut w, forward(), 60);
    assert!(w.player.crouching);
    assert!((w.player.speed - CROUCH_RUN_SPEED).abs() < 1e-3);
    assert!((CROUCH_RUN_SPEED - 2.98).abs() < 0.01);
    run(&mut w, Input { walk: true, ..forward() }, 60);
    assert!((w.player.speed - CROUCH_WALK_SPEED).abs() < 1e-3);
    assert!((CROUCH_WALK_SPEED - 1.37).abs() < 0.01);
}

#[test]
fn rolling_from_a_crouch_stays_crouched() {
    let mut w = world();
    w.step(&Input { crouch: true, ..idle() });
    w.step(&Input { dodge: DOWN, ..forward() });
    w.step(&Input { dodge: UP, ..forward() });
    assert_eq!(id(&w), Some(ActionId::CrouchRoll(Load::Medium, Dir::Front)));
    assert_eq!(id(&w).unwrap().def().source, "a000_327110");
    let start = w.player.pos;
    run(&mut w, idle(), 160);
    assert!(action(&w).is_none());
    assert!(w.player.crouching);
    assert!((w.player.pos.distance(start) - 3.57).abs() < 0.01);
}

#[test]
fn second_light_swing_carries_its_real_root_motion() {
    // This animation is authored at 60 fps; read as 30 it would be twice as
    // long and have no motion at all.
    let def = atk(AttackKind::Light2).def();
    assert_eq!(def.total, 62.0);
    assert!((def.motion_at(def.total)[2] - 1.193).abs() < 1e-3);
    let hit = def.hit.unwrap();
    assert_eq!((hit.from, hit.to), (13.0, 15.0));
}

#[test]
fn hit_reaction_depends_on_the_side_and_strength_of_the_hit() {
    use super::player::Incoming;
    let level = Level::flat();
    let react = |from: Vec3, hurt: HurtLevel| {
        let mut w = world();
        let hit = Incoming { damage: 10.0, stamina: 10.0, from, low: false, level: hurt };
        w.player.receive_hit(&hit, &level);
        id(&w).unwrap()
    };
    // The player faces +Z, with +X on their left.
    assert_eq!(react(Vec3::Z, HurtLevel::Small), ActionId::Hurt(HurtLevel::Small, Dir::Front));
    assert_eq!(react(-Vec3::Z, HurtLevel::Small), ActionId::Hurt(HurtLevel::Small, Dir::Back));
    assert_eq!(react(Vec3::X, HurtLevel::Middle), ActionId::Hurt(HurtLevel::Middle, Dir::Left));
    assert_eq!(react(-Vec3::X, HurtLevel::Middle), ActionId::Hurt(HurtLevel::Middle, Dir::Right));

    let small = ActionId::Hurt(HurtLevel::Small, Dir::Front).def();
    let middle = ActionId::Hurt(HurtLevel::Middle, Dir::Front).def();
    assert_eq!((small.source, small.total, small.cancel_move), ("a000_005110", 34.0, 15.0));
    assert_eq!((middle.source, middle.total, middle.cancel_move), ("a000_005200", 57.0, 28.0));
}

#[test]
fn nothing_gets_you_out_of_a_stagger_before_it_recovers() {
    let mut w = world();
    w.dummy.aggressive = true;
    w.player.pos = Vec3::new(0.0, 0.0, 5.0);
    while !matches!(id(&w), Some(ActionId::Hurt(..))) {
        w.step(&idle());
    }
    let def = id(&w).unwrap().def();
    // Mash roll the whole way through.
    let tap = Button { held: false, pressed: true, released: true };
    while let Some((ActionId::Hurt(..), f)) = action(&w) {
        assert!(f < def.cancel_move + DF);
        w.step(&Input { dodge: tap, ..forward() });
    }
    assert!(matches!(id(&w), Some(ActionId::Roll(..))), "the queued roll comes out as soon as it can");
}

#[test]
fn stepping_off_a_ledge_falls_and_lands_with_the_fall_landing() {
    let mut w = World::new(Level::arena());
    // On top of the 1 m crate, walking off its +Z edge.
    w.player.pos = Vec3::new(-7.0, 1.0, 5.5);
    let mut fell = false;
    for _ in 0..200 {
        w.step(&forward());
        if let State::Air(a) = w.player.state {
            assert!(!a.jumped);
            fell = true;
        }
        if id(&w) == Some(ActionId::LandFall) {
            break;
        }
    }
    assert!(fell);
    assert_eq!(id(&w), Some(ActionId::LandFall));
    assert_eq!(w.player.pos.y, 0.0);
    let def = ActionId::LandFall.def();
    assert_eq!((def.source, def.total, def.cancel_move), ("a000_029020", 17.0, 7.0));
}

#[test]
fn a_jump_off_a_ledge_turns_into_a_fall() {
    let mut w = World::new(Level::arena());
    // Jump off the top of the 1 m crate, out over its +Z edge.
    w.player.pos = Vec3::new(-7.0, 1.0, 5.8);
    w.step(&Input { jump: DOWN, ..forward() });
    let mut became_fall = false;
    for _ in 0..200 {
        w.step(&idle());
        if let State::Air(a) = w.player.state {
            assert!(a.jumped);
            became_fall |= a.falling();
        }
        if matches!(id(&w), Some(ActionId::LandFall | ActionId::LandLight | ActionId::LandRun)) {
            break;
        }
    }
    assert!(became_fall);
    assert_eq!(id(&w), Some(ActionId::LandFall), "it lands as a fall, not as a jump");

    // On flat ground the same jump never gets that far.
    let mut w = world();
    w.step(&Input { jump: DOWN, ..idle() });
    for _ in 0..200 {
        w.step(&idle());
        if let State::Air(a) = w.player.state {
            assert!(!a.falling());
        }
    }
    assert!(action(&w).is_none());
}
