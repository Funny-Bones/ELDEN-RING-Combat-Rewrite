# Elden Ring player movement and combat, in Bevy

A sandbox that recreates how the player character moves and fights in
ELDEN RING, written in Rust on [Bevy](https://bevyengine.org) 0.19.

The character is a stick-and-capsule rig, but what drives it is real: the
timings, movement and animations are read from the game's own files rather
than tuned by eye.

This is a fan project for study. It is not affiliated with or endorsed by
FromSoftware or Bandai Namco. Nothing from the game is in this repository,
only code: to build and run it you need your own copy of the game, from which
the tools here generate the data.

## What is in it

- **Movement:** walk, run, sprint, crouch, lock-on strafing, jumping, falling
  with fall damage, and stairs with per-foot ground placement.
- **Dodging:** rolls that come out on button release, backsteps, light /
  medium / heavy equip-load rolls, directional rolls while locked on, and
  crouched rolls.
- **Combat:** light chains, heavy and charged heavy attacks, running, rolling,
  backstep, crouch, jump and guard-counter attacks; guarding, guard break and
  graded, directional hit reactions.
- **Weapons:** 24 classes, each usable one- or two-handed with its own
  moveset either way: Dagger, Longsword, Claymore, Greatsword, Rapier,
  Uchigatana, Club, Battle Axe, Short Spear, Halberd, Heavy Thrusting Sword,
  Curved Sword, Curved Greatsword, Twinblade, Great Hammer, Flail, Greataxe,
  Great Spear, Reaper, Whip, Fist, Claw, Colossal Weapon and Torch, plus a
  Shield in the off hand.
- **A sparring dummy** that can be set hostile, to test dodging, blocking and
  getting hit.
- **Sound:** the game's own footsteps, cloth and armour movement, swings and
  landings, on the frames the animations call for them, chosen and mixed the
  way the game's Wwise banks say (random variations, layers, volumes).

## How faithful it is

Read from the game's files:

- Every action's length, hit window, i-frames, cancel windows and input window.
- Root motion for every action, and walk / run / sprint / crouch speeds.
- Attack stamina costs, motion values and each weapon's base attack.
- The player skeleton and the animations themselves.
- Which sounds each animation plays and on which frame, which recordings each
  sound picks from, their volumes, and each weapon's swing-sound offset.

Still estimated (all marked `ESTIMATE` in `src/sim/data.rs`):

- Roll, backstep and jump stamina costs, and how long to hold for a sprint.
- Max HP and stamina, stamina regeneration, sprint drain.
- Gravity after a jump's arc ends, and the fall-damage thresholds.
- Locomotion acceleration and turn rates.
- Hit shapes: a wedge in front of the character, not the game's capsules.

Sound is the player's alone and not positional. Where the game picks a sound
from the surroundings, the sandbox bakes one choice: dirt underfoot, leather
armour and cloth shoes (`FLOOR_MATERIAL`, `ARMOUR_MATERIAL` and `SWITCHES` in
`tools/bake_sounds.py`). Pitch and level vary a little each time, as the game
does. A few weapon classes whose swings live in banks outside `cs_main` swing
silently.

Not included: weapon skills, parrying, stat scaling, the two-handing damage
bonus, weapon-swap animations, heavier hit reactions (knockdowns, launches),
and multiple hits within one attack. The rig is primitives, so fingers, cloth
and faces are not drawn, and animation blending is simpler than the game's.

Which animation belongs to which action is partly inferred from the data,
because the game decides that in a compiled script this project does not read.

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
| Two-hand right weapon | E + left click | Y + RB |
| Two-hand left armament | E + right click | Y + LB |
| Next weapon | Right arrow | D-pad right |
| Lock on | Q or middle click | R3 |

Sandbox keys: `1` / `2` / `3` set light / medium / heavy equip load, `T` makes
the dummy hostile, `F1` toggles the i-frame tint, `H` toggles the help overlay,
`Esc` releases the mouse. Click the window to capture the mouse.

## Setup

You need Rust, Python 3.10 or newer, and Windows (the tools load the game's
own decompression DLL).

Generated files are deliberately not in the repository, because they are
derived from the game: the action table (`src/sim/extracted.rs`), the baked
animations (`assets/player_anims.bin`) and the sounds
(`assets/player_sounds.bin`, `assets/sounds/`). The project will not compile or run
until you generate them from your own game files. That is one command once
the files are unpacked.

1. **Unpack the game files.** The archives are encrypted, so this step uses
   community tools:
   - With UXM Selective Unpack, unpack only these from `chr/`:
     `c0000.anibnd.dcx`, `c0000_a00_hi`, `c0000_a00_lo`, `c0000_a00_md`,
     `c0000_a0x`, `c0000_a1x`, `c0000_a2x`, `c0000_a3x` and `c0000_a4x`
     (each `.anibnd.dcx`). Use **Unpack** only, never **Patch**.
   - With UXM Selective Unpack, also unpack `sd/cs_smain.bnk` and
     `sd/enus/cs_main.bnk`, the sound banks.
   - With WitchyBND, unpack `regulation.bin` into a `regulation-bin` folder.
   - Put the results in one folder, laid out as `chr/...`, `sd/...` and
     `regulation-bin/...`.

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

3. **Generate the data** (takes well under a minute):

   ```bash
   python tools/setup.py
   ```

4. **Run it** from the project folder:

   ```bash
   cargo run
   ```

After that the game files are no longer needed to run the sandbox, only to
regenerate the data. `setup.py` just runs the two generators, which can also
be run on their own after changing which weapons, actions or clips are used:

```bash
python tools/extract.py
```

```bash
python tools/bake_anims.py
```

```bash
python tools/bake_sounds.py
```

Without the sounds the sandbox still runs, silently.

## Layout

| Path | What it is |
|---|---|
| `src/sim/` | The whole game as a pure 60 Hz state machine, with no engine types. |
| `src/sim/extracted.rs` | Generated action table. Not in the repository; do not edit by hand. |
| `src/sim/data.rs` | Action types, plus every value that is still an estimate. |
| `src/sim/tests.rs` | Behaviour tests; run with `cargo test`. |
| `src/rig.rs`, `src/anim.rs` | The rig, and loading and playing the baked animations. |
| `src/camera.rs`, `src/input.rs`, `src/hud.rs`, `src/view.rs` | Camera, bindings, HUD, arena and dummy. |
| `tools/setup.py` | Runs both generators below. |
| `tools/extract.py` | Reads timings, root motion and params; writes `extracted.rs`. |
| `tools/bake_anims.py` | Decodes the skeletal animations; writes `assets/player_anims.bin`. |
| `tools/bake_sounds.py` | Resolves each animation's sound events in the Wwise banks; writes `assets/player_sounds.bin` and `assets/sounds/`. |
| `tools/wwise.py`, `tools/wem.py` | Wwise bank reader, and Wwise Vorbis to Ogg Vorbis conversion (a port of ww2ogg, see `tools/ww2ogg/COPYING`). |
| `src/audio.rs` | Plays the baked sounds as the animations pass their frames. |
| `tools/*.py` (others) | Readers for the game's container, event, animation and param formats. |

## A note on the data

`src/sim/extracted.rs`, `assets/player_anims.bin`, `assets/player_sounds.bin`
and `assets/sounds/` are derived from the game's files, so none is committed
and all are in `.gitignore`. Please
keep it that way in forks: share the code, and let each person generate the
data from the copy of the game they own.
