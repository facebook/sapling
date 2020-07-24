#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import json
import os
import subprocess
import sys
from os.path import abspath, dirname, join


parser = argparse.ArgumentParser(
    description="Run Mononoke integration tests from getdeps.py build"
)
parser.add_argument(
    "install_dir",
    help="Location of getdeps.py install dir (With installed mononoke and eden_scm projects)",
)
parser.add_argument(
    "build_dir", help="Location where to put generated manifest.json file"
)
args = parser.parse_args()

install_dir = args.install_dir
build_dir = args.build_dir
repo_root = dirname(dirname(dirname(dirname(dirname(abspath(__file__))))))

exec(open(join(repo_root, "eden/mononoke/tests/integration/manifest_deps"), "r").read())

MANIFEST_DEPS = {}
for k, v in OSS_DEPS.items():  # noqa: F821
    if v.startswith("//"):
        MANIFEST_DEPS[k] = join(repo_root, v[2:])
    else:
        MANIFEST_DEPS[k] = v
for k, v in MONONOKE_BINS.items():  # noqa: F821
    MANIFEST_DEPS[k] = join(install_dir, "mononoke/bin", v)
for k, v in EDENSCM_BINS.items():  # noqa: F821
    MANIFEST_DEPS[k] = join(install_dir, "eden_scm/bin", v)

os.makedirs(build_dir, exist_ok=True)
with open(join(build_dir, "manifest.json"), "w") as f:
    f.write(json.dumps(MANIFEST_DEPS, sort_keys=True, indent=4))

tests = ["test-init.t"]

env = dict(os.environ.items())
env["NO_LOCAL_PATHS"] = "1"
eden_scm_packages = join(install_dir, "eden_scm/lib/python2.7/site-packages")
pythonpath = env.get("PYTHONPATH")
env["PYTHONPATH"] = eden_scm_packages + (":{}".format(pythonpath) if pythonpath else "")

sys.exit(
    subprocess.run(
        [
            sys.executable,
            join(
                repo_root, "eden/mononoke/tests/integration/integration_runner_real.py"
            ),
            join(build_dir, "manifest.json"),
        ]
        + tests,
        env=env,
    ).returncode
)
