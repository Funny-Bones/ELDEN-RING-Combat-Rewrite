"""Generates everything the sandbox needs from your own copy of the game:

  src/sim/extracted.rs      action timings, root motion, weapon data
  assets/player_anims.bin   the baked animations
  assets/player_sounds.bin  which sounds each animation plays, and when
  assets/sounds/            the recordings, converted to Ogg Vorbis

Neither is distributed with the project. Unpack the game files first and
point ER_FILES / ER_GAME_DIR at them (see README), then run:

    python tools/setup.py
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import paths

# Fail on a wrong folder before doing any work, with a message that says which.
paths.er_files()
paths.game_dir()

import bake_anims
import bake_sounds
import extract

print("1/3  Extracting action data...")
extract.main()
print("2/3  Baking animations...")
bake_anims.main()
# Sound is optional: without the banks the sandbox runs silent.
missing = bake_sounds.missing_banks()
if missing:
    print("3/3  Skipping sounds: %s not unpacked (see README). The sandbox will run silent." % ", ".join(missing))
else:
    print("3/3  Baking sounds...")
    bake_sounds.main()
print("\nDone. Start the sandbox with:  cargo run")
