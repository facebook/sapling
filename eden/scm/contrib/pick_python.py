# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Pick a Python that is more likely to be able to build with setup.py.

Arguments: Binary name (ex. python3).
Stdout: Full path to a system python, or the first argument of the input.

This script does not need to be executed using a system Python.
"""

import ast
import os
import subprocess
import sys


def main(args):
    dirs = os.environ.get("PATH").split(os.pathsep)
    names = args or ["python3"]
    if names == ["python3"]:
        # Try different pythons
        names = ["python3.8", "python3.10", "python3.7", "python3.6"] + names
    for name in names:
        for dir in dirs:
            path = os.path.join(dir, name)
            if does_python_look_good(path):
                print(path)
                return

    # Fallback
    sys.stderr.write("warning: cannot find a proper Python\n")
    sys.stdout.write(names[0])


def does_python_look_good(path):
    if not os.path.isfile(path):
        return False
    try:
        cfg = ast.literal_eval(
            subprocess.check_output(
                [path, "-c", "import sysconfig;print(sysconfig.get_config_vars())"]
            ).decode("utf-8")
        )
        cflags = cfg["CFLAGS"]
        if "-nostdinc" in cflags.split():
            sys.stderr.write("%s: ignored, lack of C stdlib\n" % path)
            return False
        includepy = cfg["INCLUDEPY"]
        if not os.path.exists(os.path.join(includepy, "Python.h")):
            sys.stderr.write(
                "%s: ignored, missing Python.h in %s\n" % (path, includepy)
            )
            return False
        realpath = subprocess.check_output(
            [path, "-c", "import sys;print(sys.executable)"]
        )
        if b"fbprojects" in realpath:
            sys.stderr.write(
                "%s: ignored, avoid using the fb python for non-fb builds\n" % path
            )
            return False
        return True
    except Exception:
        return False


if __name__ == "__main__":
    code = main(sys.argv[1:]) or 0
    sys.stderr.flush()
    sys.stdout.flush()
    sys.exit(code)
