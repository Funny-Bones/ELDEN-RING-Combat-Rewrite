"""Where the tools look for game data. Both locations come from environment
variables so nothing machine-specific lives in the repository.

  ER_FILES     folder holding the unpacked files (`chr/`, `regulation-bin/`)
  ER_GAME_DIR  the game's own `Game` folder, for its Oodle DLL
"""
import os
import sys
from pathlib import Path

if sys.platform.startswith("win"):
    DEFAULT_GAME_DIR = r"C:\Program Files (x86)\Steam\steamapps\common\ELDEN RING\Game"
elif sys.platform == "darwin":
    DEFAULT_GAME_DIR = os.path.expanduser("~/Library/Application Support/Steam/steamapps/common/ELDEN RING/Game")
else:
    DEFAULT_GAME_DIR = os.path.expanduser("~/.local/share/Steam/steamapps/common/ELDEN RING/Game")


def _folder(variable, default, must_contain):
    path = Path(os.environ.get(variable, default))
    if not (path / must_contain).exists():
        sys.exit(
            f"{variable} is {'set to' if variable in os.environ else 'not set; tried'} {path}\n"
            f"but there is no {must_contain} in it. Point {variable} at the right folder (see README)."
        )
    return path


def er_files():
    return _folder("ER_FILES", "er-files", "chr")


def game_dir():
    return _folder("ER_GAME_DIR", DEFAULT_GAME_DIR, "oo2core_6_win64.dll")
