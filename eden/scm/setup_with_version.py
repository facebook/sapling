# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# A wrapper script that is responsible for computing the version and propagating
# it through to the underlying setup.py script via the SAPLING_VERSION
# environment variable.

import os
import sys

# If we're executing inside an embedded Python instance, it won't load
# modules outside the embedded python. So let's add our directory manually,
# before we import things.
sys.path.append(os.path.dirname(os.path.realpath(__file__)))

import hashlib
import struct
import subprocess

from setup_utils import hgtemplate


def pickversion():
    version = os.environ.get("SAPLING_VERSION")
    if version:
        return version

    # New version system: YYMMDD_HHmmSS_hash
    # This is duplicated a bit from build_rpm.py:auto_release_str()
    template = '{sub("([:+-]|\d\d\d\d$)", "",date|isodatesec)} {node|short}'
    # if hg is not found, fallback to a fixed version
    out = hgtemplate(template) or ""
    # Some tools parse this number to figure out if they support this version of
    # Mercurial, so prepend with 4.4.2.
    # ex. 4.4.2_20180105_214829_58fda95a0202
    return "_".join(["4.4.2"] + out.split())


version = pickversion()
if not isinstance(version, str):
    version = version.decode("ascii")

versionb = version.encode("ascii")
# calculate a versionhash, which is used by chg to make sure the client
# connects to a compatible server.
versionhash = str(struct.unpack(">Q", hashlib.sha1(versionb).digest()[:8])[0])

env = os.environ.copy()
env["SAPLING_VERSION"] = version
env["SAPLING_VERSION_HASH"] = versionhash
# Somehow `HOMEBREW_CCCFG` gets set every time setup.py runs when running on
# Homebrew. This affects certain Rust targets, somehow, making them produce
# a target of the wrong arch (e.g. cross compiling to arm64 from x86)
env.pop("HOMEBREW_CCCFG", None)

python = env.get("PYTHON_SYS_EXECUTABLE", sys.executable)
p = subprocess.run([python, "setup.py"] + sys.argv[1:], env=env)
sys.exit(p.returncode)
