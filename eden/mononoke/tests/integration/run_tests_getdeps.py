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
from glob import iglob
from os.path import abspath, basename, dirname, join
from pathlib import Path


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
parser.add_argument(
    "tests", nargs="*", help="Optional list of tests to run. Run all if none provided"
)
parser.add_argument(
    "-r",
    "--rerun-failed",
    action="store_true",
    help="Rerun failed tests based on '.testfailed' file",
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

if args.tests or args.rerun_failed:
    tests = {basename(p) for p in args.tests or []}

    if args.rerun_failed:
        # Based on eden/scm/tests/run-tests.py
        for title in ("failed", "errored"):
            failed = Path(repo_root) / "eden/mononoke/tests/integration/.test{}".format(
                title
            )
            if failed.is_file():
                tests.update(t for t in failed.read_text().splitlines() if t)

    tests = list(tests)
else:
    excluded_tests = {
        "test-backfill-git-mapping.t",  # "${ENV^^}" problem
        "test-backsync-forever.t",  # Unknown issue
        "test-backsyncer-merges.t",  # Missing BACKSYNCER
        "test-blobimport-derived-data.t",  # "|&" problem
        "test-blobimport-inline.t",  # Illegal byte sequence and "sed -i"
        "test-blobimport-lfs.t",  # Timed out
        "test-blobimport.t",  # Case insensitivity of paths in MacOS
        "test-blobstore_healer.t",  # PANIC not implemented in sql_ext
        "test-bookmarks-filler.t",  # Probably missing binary
        "test-cmd-manual-scrub.t",  # Just wrong outout
        "test-cross-repo-commit-sync-live.t",  # Unknown issue
        "test-cross-repo-commit-sync-merge.t",  # requires FB-specific command
        "test-cross-repo-commit-sync.t",  # requires FB-specific command
        "test-cross-repo-commit-validator.t",  # requires FB-specific command
        "test-edenapi-server-complete-trees.t",  # Missing eden/scm's commands
        "test-edenapi-server-files.t",  # Missing eden/scm's commands
        "test-edenapi-server-history.t",  # Missing eden/scm's commands
        "test-edenapi-server-trees.t",  # Missing eden/scm's commands
        "test-fastreplay-inline-args.t",  # Returns different data in OSS
        "test-fastreplay-remote-args.t",  # "grep -A" problem
        "test-gitimport-octopus.t",  # Missing MONONOKE_REGENERATE_HG_FILENODES
        "test-gitimport.t",  # Issue with hggit extension
        "test-hook-tailer.t",  # Issue with hggit extension
        "test-hooks.t",  # Hooks are not in OSS yet
        "test-inconsistent-hash.t",  # Illegal byte sequence and "sed -i"
        "test-infinitepush-hydrated.t",  # "|&" problem
        "test-infinitepush-lfs.t",  # Timed out
        "test-large-path-and-content.t",  # Complex bash issues
        "test-lfs-copytracing.t",  # Timed out
        "test-lfs-server-acl-check.t",  # Timed out
        "test-lfs-server-consistent-hashing.t",  # Timed out
        "test-lfs-server-disabled-hostname-resolution.t",  # Timed out
        "test-lfs-server-identity-parsing-from-header.t",  # Timed out
        "test-lfs-server-identity-parsing-untrusted.t",  # Timed out
        "test-lfs-server-identity-parsing.t",  # Timed out
        "test-lfs-server-max-upload-size.t",  # Timed out
        "test-lfs-server-proxy-skip-upstream.t",  # Complex bash issues
        "test-lfs-server-proxy-sync.t",  # Timed out
        "test-lfs-server-proxy.t",  # Timed out
        "test-lfs-server-rate-limiting.t",  # Timed out
        "test-lfs-server-scuba-logging.t",  # Timed out
        "test-lfs-server.t",  # Timed out
        "test-lfs-to-mononoke.t",  # Timed out
        "test-lfs-wantslfspointers.t",  # Timed out
        "test-lfs.t",  # Timed out
        "test-megarepo-tool.t",  # Missing MONONOKE_HG_SYNC
        "test-mononoke-admin.t",  # Missing MEGAREPO_TOOL
        "test-mononoke-hg-sync-job-generate-bundles-force.t",  # Missing MONONOKE_HG_SYNC
        "test-mononoke-hg-sync-job-generate-bundles-lfs-verification.t",  # Timed out
        "test-mononoke-hg-sync-job-generate-bundles-lfs.t",  # Timed out
        "test-mononoke-hg-sync-job-generate-bundles-loop.t",  # Missing MONONOKE_HG_SYNC
        "test-mononoke-hg-sync-job-generate-bundles-other-books.t",  # Missing MONONOKE_HG_SYNC
        "test-mononoke-hg-sync-job-generate-bundles.t",  # Missing MONONOKE_HG_SYNC
        "test-mononoke-hg-sync-job-sync-globalrevs.t",  # Missing MONONOKE_HG_SYNC
        "test-mononoke-hg-sync-job-with-copies.t",  # Missing MONONOKE_HG_SYNC
        "test-mononoke-hg-sync-job.t",  # Missing MONONOKE_HG_SYNC
        "test-push-protocol-lfs.t",  # Timed out
        "test-push-redirector-pushrebase-hooks.t",  # Hooks are not in OSS yet
        "test-push-redirector-pushrebase-onesided.t",  # Missing MONONOKE_X_REPO_SYNC
        "test-push-redirector-pushrebase.t",  # "[[ -v ... ]]" in library-push-redirector.sh and "ls" problem
        "test-push-redirector-sync-job.t",  # Missing BACKSYNCER
        "test-pushrebase-block-casefolding.t",  # Most likely MacOS path case insensitivity
        "test-pushrebase-discovery.t",  # Hooks are not in OSS yet
        "test-remotefilelog-lfs.t",  # Timed out
        "test-scs-blame.t",  # Missing SCS_SERVER
        "test-scs-common-base.t",  # Missing SCS_SERVER
        "test-scs-diff.t",  # Missing SCS_SERVER
        "test-scs-list-bookmarks.t",  # Missing SCS_SERVER
        "test-scs-log.t",  # Missing SCS_SERVER
        "test-scs-lookup.t",  # Missing SCS_SERVER
        "test-scs-x-repo.t",  # Missing SCS_SERVER
        "test-scs.t",  # Missing SCS_SERVER
        "test-server.t",  # Returns different data in OSS
        "test-traffic-replay.t",  # Missing TRAFFIC_REPLAY
        "test-unbundle-replay-hg-bookmark.t",  # "kill" printing problem
        "test-unbundle-replay-hg-recording.t",  # Returns different data in OSS
        "test-walker-throttle.t",  # "date" and arithmetic problem
    }

    tests = [
        t
        for t in (
            basename(p)
            for p in iglob(join(repo_root, "eden/mononoke/tests/integration/*.t"))
        )
        if t not in excluded_tests
    ]

env = dict(os.environ.items())
env["NO_LOCAL_PATHS"] = "1"
eden_scm_packages = join(install_dir, "eden_scm/lib/python2.7/site-packages")
pythonpath = env.get("PYTHONPATH")
env["PYTHONPATH"] = eden_scm_packages + (":{}".format(pythonpath) if pythonpath else "")

if tests:
    sys.exit(
        subprocess.run(
            [
                sys.executable,
                join(
                    repo_root,
                    "eden/mononoke/tests/integration/integration_runner_real.py",
                ),
                join(build_dir, "manifest.json"),
            ]
            + tests,
            env=env,
        ).returncode
    )
