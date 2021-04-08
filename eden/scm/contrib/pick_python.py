# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Pick a Python that is more likely to be able to build with setup.py.

Arguments: Binary name (ex. python2, or python3).
Stdout: Full path to a system python, or the first argument of the input.

This script does not need to be executed using a system Python.
"""

import os
import subprocess
import sys


def main(args):
    dirs = os.environ.get("PATH").split(os.pathsep)
    names = args or ["python3"]
    for dir in dirs:
        for name in names:
            path = os.path.join(dir, name)
            if does_python_look_good(path):
                sys.stdout.write(path)
                return
    # Fallback
    sys.stdout.write(names[0])


def does_python_look_good(path):
    if not os.path.isfile(path):
        return False
    try:
        cflags = subprocess.check_output(
            [path, "-c", "import sysconfig;print(sysconfig.get_config_var('CFLAGS'))"]
        )
        if b"-nostdinc" in cflags.split():
            sys.stderr.write("%s: ignored, lack of C stdlib\n" % path)
            return False
        return True
    except Exception:
        return False


if __name__ == "__main__":
    code = main(sys.argv[1:]) or 0
    sys.stderr.flush()
    sys.stdout.flush()
    sys.exit(code)
