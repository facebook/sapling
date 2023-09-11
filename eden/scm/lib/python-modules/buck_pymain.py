# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Pick the "right" python to execute the script in stdin.
# This should match the python linked with "hgmain" in buck build.
import os
import runpy
import subprocess
import sys

argv = sys.argv
assert len(argv) == 1
code = sys.stdin.read()

# Right now, hgmain links with Python 3.8
wanted_version = (3, 8)
current_version = sys.version_info[:2]
if current_version != wanted_version:
    m, n = wanted_version
    # Sometimes buck's python_binary does not use the Python we want
    # (that matches the Rust main executable).
    # Try to find system python in various places...
    candidates = [
        f"/usr/local/fbcode/platform010/bin/python{m}.{n}",
        f"/opt/homebrew/bin/python{m}.{n}",
        f"\\Python{m}{n}\\Python.exe",
        f"/usr/local/bin/python{m}.{n}",
        f"/usr/bin/python{m}.{n}",
    ]
    for path in candidates:
        if os.path.exists(path):
            subprocess.run([path], check=True)
            sys.exit()

    raise RuntimeError(
        f"Python at build time ({current_version}) is different from Python used by the executable ({wanted_version})"
    )

exec(code)
