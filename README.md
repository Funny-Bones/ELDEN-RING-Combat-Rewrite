# Elden Ring player movement and combat, in Bevy

A sandbox that recreates how the player character moves and fights in
ELDEN RING, written in Rust on [Bevy](https://bevyengine.org) 0.19.

The character is a stick-and-capsule rig, but what drives it is real: the
timings, movement and animations are read from the game's own files rather
than tuned by eye.

https://github.com/user-attachments/assets/b58dbb0d-a3db-46d5-82ee-d9152503e40c

*The sandbox playing itself through everything it does.
[Watch it in full quality on YouTube](https://www.youtube.com/watch?v=3o1tS0Gufdk).*

This is a fan project for study. It is not affiliated with or endorsed by
FromSoftware or Bandai Namco. Nothing from the game is in this repository,
only code: to build and run it you need your own copy of the game, from which
the tools here generate the data.

## What is in it

### Movement

- Walk, run and sprint, with the braking animation when you stop from a run.
- Crouching, with its own idle, walk, run, stop, and the animations for
  crouching down and standing back up.
- Lock-on: strafing around the target in four directions while facing it.
- Jumps from a standstill, a walk, a run and a sprint. Locked on, walking and
  running jumps go forward, back, left or right while you keep facing the
  target, and so do their landings.
- Falling: a separate start when you walk off a ledge, light and heavy
  landings, fall damage and fall death.
- Stairs and slopes, with each foot placed on the ground under it.

### Dodging

- Rolls come out when the button is released; holding it sprints instead.
- Light, medium and heavy equip-load rolls, backsteps and crouched rolls.
- Rolls in four directions while locked on.

### Combat

- Light attack chains, heavy and charged heavy attacks.
- Running, rolling, backstep, crouch, jump and guard-counter attacks.
- Attacks with several hits land every one of them, each with its own damage
  and stamina cost.
- Guarding, guard hits and guard break.
- Hit reactions in four strengths and four directions: a flinch, a stagger, a
  large stagger, and a knockdown that throws you back, keeps you invincible
  while you are down and lets you roll out early.
- Changing grip or weapon plays on the upper body while you keep moving.

### Weapons

24 classes, each usable one- or two-handed with its own moveset either way:
Dagger, Longsword, Claymore, Greatsword, Rapier, Uchigatana, Club, Battle Axe,
Short Spear, Halberd, Heavy Thrusting Sword, Curved Sword, Curved Greatsword,
Twinblade, Great Hammer, Flail, Greataxe, Great Spear, Reaper, Whip, Fist,
Claw, Colossal Weapon and Torch, plus a Shield in the off hand that can be
two-handed too.

### The sparring dummy

It stands still until you press `T`. Hostile, it cycles through four attacks,
one for each hit reaction:

| Attack | Telegraph | Reaction |
|---|---|---|
| Overhead slam | orange | stagger |
| Low sweep (jump it) | blue | flinch |
| Harder slam | red | large stagger |
| Hardest slam | purple | knockdown |

### The arena

Crates stacked into a climb up to a wall you can walk along, a platform
across a gap from it, a slope, a row of pillars, a low dais, and a staircase
21 m high that passes every fall-damage threshold on the way up.

### The demo

Press `Enter` and the sandbox plays itself through everything above, with a
caption for each thing it shows: about five minutes, made for recording.
`Enter` again stops it. It resets the arena when it starts, and plays through
the same inputs a player has, so nothing in it is staged.

### Sound (optional)

With the game's sound banks unpacked, the sandbox plays the game's own
footsteps, cloth and armour movement, swings and landings, on the frames the
animations call for them, chosen and mixed the way the Wwise banks say.
Without the banks it runs silent.

## How faithful it is

Read from the game's files:

- Every action's length, hit windows, i-frames, cancel windows and input window.
- Root motion for every action, and walk / run / sprint / crouch speeds.
- Stamina costs (each hit of an attack, rolls, backsteps, jumps), motion
  values and each weapon's base attack.
- Max HP and stamina: the Vagabond starting class at its starting level,
  through the game's stat curves.
- Changing grip or weapon: its animations and when the change takes effect.
- The player skeleton and the animations themselves.
- Which sounds each animation plays and on which frame, which recordings each
  sound picks from, their volumes, and each weapon's swing-sound offset.

Still estimated (all marked `ESTIMATE` in `src/sim/data.rs`):

- How long the dodge button must be held for a sprint.
- Stamina regeneration and sprint drain.
- Gravity after a jump's arc ends, and the fall-damage thresholds.
- Locomotion acceleration and turn rates.
- Hit shapes: a wedge in front of the character, not the game's capsules.
- The dummy: its attacks, damage and timing are made up for testing.

Which animation belongs to which action is partly inferred from the data,
because the game decides that in a compiled script this project does not
read. Two inferences worth knowing about:

- Which large-stagger animation answers a hit from which side is picked from
  the way the head snaps.
- An attack that follows a running, rolling, backstep or crouch attack goes
  straight into the second light attack. The game has short transition clips
  there, but they carry no timing of their own.

Not included: weapon skills, parrying, stat scaling, the two-handing damage
bonus, and being launched into the air by a hit. The rig is primitives, so
fingers, cloth and faces are not drawn, and animation blending is simpler
than the game's.

Sound is the player's alone and not positional. Where the game picks a sound
from the surroundings, the sandbox bakes one choice: dirt underfoot, leather
armour and cloth shoes (`FLOOR_MATERIAL`, `ARMOUR_MATERIAL` and `SWITCHES` in
`tools/bake_sounds.py`). Pitch and level vary a little each time, as the game
does. A few weapon classes whose swings live in banks outside `cs_main` swing
silently.

## Controls

| Action | Keyboard / mouse | Gamepad |
|---|---|---|
| Move / look | WASD / mouse | Left stick / right stick |
| Walk | Left Alt | Tilt lightly |
| Roll or backstep | Tap Space | Tap B |
| Sprint | Hold Space | Hold B |
| Jump | F | A |
| Crouch | X | L3 |
| Light attack | Left click | RB |
| Heavy attack (hold to charge) | Shift + left click | RT |
| Guard | Right click | LB |
| Guard counter | Shift + left click after blocking a hit | RT after blocking a hit |
| Two-hand right weapon | E + left click | Y + RB |
| Two-hand left armament | E + right click | Y + LB |
| Next weapon | Right arrow | D-pad right |
| Lock on | Q or middle click | R3 |

Sandbox keys: `1` / `2` / `3` set light / medium / heavy equip load, `T` makes
the dummy hostile, `F1` toggles the i-frame tint, `H` toggles the help overlay,
`Enter` plays the demo,
`Esc` releases the mouse. Click the window to capture the mouse.

## Setup

You need Rust, Python 3.10 or newer, and Windows (the tools load the game's
own decompression DLL).

Generated files are deliberately not in the repository, because they are
derived from the game: the action table (`src/sim/extracted.rs`), the baked
animations (`assets/player_anims.bin`) and the sounds
(`assets/player_sounds.bin`, `assets/sounds/`). The project will not compile
or run until you generate the first two from your own game files. That is one
command once the files are unpacked.

1. **Unpack the game files.** The archives are encrypted, so this step uses
   community tools:
   - With UXM Selective Unpack, unpack only these from `chr/`:
     `c0000.anibnd.dcx`, `c0000_a00_hi`, `c0000_a00_lo`, `c0000_a00_md`,
     `c0000_a0x`, `c0000_a1x`, `c0000_a2x`, `c0000_a3x` and `c0000_a4x`
     (each `.anibnd.dcx`). Use **Unpack** only, never **Patch**.
   - With WitchyBND, unpack `regulation.bin` into a `regulation-bin` folder.
   - Optional, for sound: also unpack `sd/cs_smain.bnk` and
     `sd/enus/cs_main.bnk`.
   - Put the results in one folder, laid out as `chr/...`, `regulation-bin/...`
     and, if you have them, `sd/...`.

2. **Tell the tools where things are.** They read two environment variables:

   | Variable | Points at | Default |
   |---|---|---|
   | `ER_FILES` | the folder from step 1 | `er-files` in the project |
   | `ER_GAME_DIR` | the game's `Game` folder | the default Steam location |

   In PowerShell, for example:

   ```powershell
   $env:ER_FILES = "C:\path\to\unpacked"
   $env:ER_GAME_DIR = "D:\Steam\steamapps\common\ELDEN RING\Game"
   ```

3. **Generate the data** (takes about a minute):

   ```bash
   python tools/setup.py
   ```

   If the sound banks are not there, it says so, skips that step and the
   sandbox runs silent.

4. **Run it** from the project folder:

   ```bash
   cargo run
   ```

After that the game files are no longer needed to run the sandbox, only to
regenerate the data. Run `setup.py` again after pulling changes that touch
`tools/`: new actions and animations need regenerated data. It just runs the
three generators, which can also be run on their own:

```bash
python tools/extract.py
```

```bash
python tools/bake_anims.py
```

```bash
python tools/bake_sounds.py
```

## Layout

| Path | What it is |
|---|---|
| `src/sim/` | The whole game as a pure 60 Hz state machine, with no engine types. |
| `src/sim/extracted.rs` | Generated action table. Not in the repository; do not edit by hand. |
| `src/sim/data.rs` | Action types, plus every value that is still an estimate. |
| `src/sim/player.rs`, `src/sim/dummy.rs`, `src/sim/level.rs` | The player, the sparring dummy and the arena. |
| `src/sim/tests.rs` | Behaviour tests; run with `cargo test`. |
| `src/rig.rs`, `src/anim.rs` | The rig, and loading and playing the baked animations. |
| `src/audio.rs` | Plays the baked sounds as the animations pass their frames. |
| `src/demo.rs` | The scripted demo, and a test that plays it through without a window. |
| `src/camera.rs`, `src/input.rs`, `src/hud.rs`, `src/view.rs` | Camera, bindings, HUD, arena and dummy visuals. |
| `tools/setup.py` | Runs the generators below. |
| `tools/extract.py` | Reads timings, root motion and params; writes `extracted.rs`. |
| `tools/bake_anims.py` | Decodes the skeletal animations; writes `assets/player_anims.bin`. |
| `tools/bake_sounds.py` | Resolves each animation's sound events in the Wwise banks; writes `assets/player_sounds.bin` and `assets/sounds/`. |
| `tools/wwise.py`, `tools/wem.py` | Wwise bank reader, and Wwise Vorbis to Ogg Vorbis conversion (a port of ww2ogg, see `tools/ww2ogg/COPYING`). |
| `tools/*.py` (others) | Readers for the game's container, event, animation and param formats. |

## A note on the data

`src/sim/extracted.rs`, `assets/player_anims.bin`, `assets/player_sounds.bin`
and `assets/sounds/` are derived from the game's files, so none is committed
and all are in `.gitignore`. Please keep it that way in forks: share the code,
and let each person generate the data from the copy of the game they own.
