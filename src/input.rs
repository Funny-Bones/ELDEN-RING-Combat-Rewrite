//! Keyboard, mouse and gamepad, mapped to Elden Ring's default bindings.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::camera::CamRig;
use crate::sim::data::Load;
use crate::sim::player::Button;
use crate::{Options, Pending, Sim};

fn merge(button: &mut Button, held: bool, pressed: bool, released: bool) {
    button.held = held;
    button.pressed |= pressed;
    button.released |= released;
}

pub fn grab_cursor(
    mut cursor: Single<&mut CursorOptions>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if mouse.just_pressed(MouseButton::Left) && cursor.grab_mode == CursorGrabMode::None {
        cursor.visible = false;
        cursor.grab_mode = CursorGrabMode::Locked;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.visible = true;
        cursor.grab_mode = CursorGrabMode::None;
    }
}

pub fn gather(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    cursor: Single<&CursorOptions>,
    cam: Res<CamRig>,
    mut pending: ResMut<Pending>,
    // The click that started as Shift+click stays a heavy attack until it is
    // released, so the charge survives letting go of Shift.
    mut heavy_click: Local<bool>,
    // The click that captured the mouse is not also an attack.
    mut was_grabbed: Local<bool>,
) {
    let inp = &mut pending.0;
    let grabbed = *was_grabbed;
    *was_grabbed = cursor.grab_mode != CursorGrabMode::None;

    let key = |code: KeyCode| keys.pressed(code) as i32 as f32;
    let mut mv = Vec2::new(
        key(KeyCode::KeyD) - key(KeyCode::KeyA),
        key(KeyCode::KeyW) - key(KeyCode::KeyS),
    );

    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    // Holding E turns the attack and guard buttons into "two-hand this side".
    let swap = keys.pressed(KeyCode::KeyE);
    let two_hand_right = grabbed && swap && mouse.just_pressed(MouseButton::Left);
    let two_hand_left = grabbed && swap && mouse.just_pressed(MouseButton::Right);
    let click = grabbed && !swap && mouse.just_pressed(MouseButton::Left);
    let unclick = mouse.just_released(MouseButton::Left);
    if click && shift {
        *heavy_click = true;
    }
    let mut light = (false, click && !shift, false);
    let mut heavy = (
        *heavy_click && mouse.pressed(MouseButton::Left),
        click && shift,
        *heavy_click && unclick,
    );
    if unclick {
        *heavy_click = false;
    }

    let mut dodge = (
        keys.pressed(KeyCode::Space),
        keys.just_pressed(KeyCode::Space),
        keys.just_released(KeyCode::Space),
    );
    let mut jump = (keys.pressed(KeyCode::KeyF), keys.just_pressed(KeyCode::KeyF), false);
    let mut guard = (
        grabbed && !swap && mouse.pressed(MouseButton::Right),
        grabbed && !swap && mouse.just_pressed(MouseButton::Right),
        false,
    );
    let mut crouch = keys.just_pressed(KeyCode::KeyX);
    let mut lock = keys.just_pressed(KeyCode::KeyQ) || mouse.just_pressed(MouseButton::Middle);

    let mut two_hand = (two_hand_right, two_hand_left);
    let mut next_weapon = keys.just_pressed(KeyCode::ArrowRight);
    let mut next_left = keys.just_pressed(KeyCode::ArrowLeft);

    for pad in &gamepads {
        // Triangle / Y is the pad's version of holding E.
        let swap = pad.pressed(GamepadButton::North);
        if swap {
            two_hand.0 |= pad.just_pressed(GamepadButton::RightTrigger);
            two_hand.1 |= pad.just_pressed(GamepadButton::LeftTrigger);
        }
        next_weapon |= pad.just_pressed(GamepadButton::DPadRight);
        next_left |= pad.just_pressed(GamepadButton::DPadLeft);
        let stick = pad.left_stick();
        if stick.length() > mv.length() {
            mv = stick;
        }
        let add = |slot: &mut (bool, bool, bool), button: GamepadButton| {
            slot.0 |= pad.pressed(button);
            slot.1 |= pad.just_pressed(button);
            slot.2 |= pad.just_released(button);
        };
        add(&mut dodge, GamepadButton::East);
        add(&mut jump, GamepadButton::South);
        if !swap {
            add(&mut light, GamepadButton::RightTrigger);
            add(&mut guard, GamepadButton::LeftTrigger);
        }
        add(&mut heavy, GamepadButton::RightTrigger2);
        crouch |= pad.just_pressed(GamepadButton::LeftThumb);
        lock |= pad.just_pressed(GamepadButton::RightThumb);
    }

    inp.mv = mv.clamp_length_max(1.0);
    inp.cam_yaw = cam.yaw;
    inp.walk = keys.pressed(KeyCode::AltLeft);
    merge(&mut inp.dodge, dodge.0, dodge.1, dodge.2);
    merge(&mut inp.jump, jump.0, jump.1, jump.2);
    merge(&mut inp.light, light.0, light.1, light.2);
    merge(&mut inp.heavy, heavy.0, heavy.1, heavy.2);
    merge(&mut inp.guard, guard.0, guard.1, guard.2);
    inp.crouch |= crouch;
    inp.lock |= lock;
    inp.two_hand_right |= two_hand.0;
    inp.two_hand_left |= two_hand.1;
    inp.next_weapon |= next_weapon;
    inp.next_left |= next_left;
}

/// Sandbox controls that are not part of the game's own bindings.
pub fn debug_keys(keys: Res<ButtonInput<KeyCode>>, mut sim: ResMut<Sim>, mut options: ResMut<Options>) {
    for (code, load) in [
        (KeyCode::Digit1, Load::Light),
        (KeyCode::Digit2, Load::Medium),
        (KeyCode::Digit3, Load::Heavy),
    ] {
        if keys.just_pressed(code) {
            sim.0.player.load = load;
        }
    }
    if keys.just_pressed(KeyCode::KeyT) {
        sim.0.dummy.aggressive = !sim.0.dummy.aggressive;
    }
    if keys.just_pressed(KeyCode::F1) {
        options.show_iframes = !options.show_iframes;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        options.show_help = !options.show_help;
    }
}
