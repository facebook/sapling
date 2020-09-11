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
from enum import Enum
from glob import iglob
from os.path import abspath, basename, dirname, join
from pathlib import Path
from sys import platform


class TestGroup(Enum):
    PASSING = "passing"
    TIMING_OUT = "timing_out"
    REQUIRING_SCS = "requiring_scs"
    FLAKY = "flaky"
    BROKEN = "broken"
    ALL = "all"

    def __str__(self):
        return self.value


def parse_args():
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
        "tests",
        nargs="*",
        help="Optional list of tests to run. If provided the --tests default is None",
    )
    parser.add_argument(
        "-t",
        "--test-groups",
        type=TestGroup,
        nargs="*",
        choices=list(TestGroup),
        help=f"Choose groups of tests to run, default: [{TestGroup.PASSING}]",
    )
    parser.add_argument(
        "-r",
        "--rerun-failed",
        action="store_true",
        help="Rerun failed tests based on '.testfailed' file",
    )
    return parser.parse_args()


def prepare_manifest_deps(install_dir, build_dir, repo_root):
    exec(
        "global OSS_DEPS; global MONONOKE_BINS; global EDENSCM_BINS; "
        + open(
            join(repo_root, "eden/mononoke/tests/integration/manifest_deps"), "r"
        ).read()
    )

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


def get_test_groups(repo_root):
    test_groups = {
        TestGroup.TIMING_OUT: {
            "test-blobimport-lfs.t",
            "test-infinitepush-lfs.t",
            "test-large-path-and-content.t",
            "test-lfs-copytracing.t",
            "test-lfs-server-acl-check.t",
            "test-lfs-server-consistent-hashing.t",
            "test-lfs-server-disabled-hostname-resolution.t",
            "test-lfs-server-identity-parsing-from-header.t",
            "test-lfs-server-identity-parsing-untrusted.t",
            "test-lfs-server-identity-parsing.t",
            "test-lfs-server-max-upload-size.t",
            "test-lfs-server-proxy-sync.t",
            "test-lfs-server-proxy.t",
            "test-lfs-server-rate-limiting.t",
            "test-lfs-server-scuba-logging.t",
            "test-lfs-server.t",
            "test-lfs-to-mononoke.t",
            "test-lfs-wantslfspointers.t",
            "test-lfs.t",
            "test-mononoke-hg-sync-job-generate-bundles-lfs-verification.t",
            "test-mononoke-hg-sync-job-generate-bundles-lfs.t",
            "test-push-protocol-lfs.t",
            "test-remotefilelog-lfs.t",
        },
        TestGroup.FLAKY: {"test-walker-count-objects.t", "test-walker-error-as-data.t"},
        TestGroup.BROKEN: {
            "test-backsync-forever.t",  # Unknown issue
            "test-bookmarks-filler.t",  # Probably missing binary
            "test-cmd-manual-scrub.t",  # Just wrong output
            "test-edenapi-server-commit-location-to-hash.t",  # Missing eden/scm's commands
            "test-edenapi-server-commit-revlog-data.t",  # Missing eden/scm's commands
            "test-edenapi-server-complete-trees.t",  # Missing eden/scm's commands
            "test-edenapi-server-files.t",  # Missing eden/scm's commands
            "test-edenapi-server-history.t",  # Missing eden/scm's commands
            "test-edenapi-server-trees.t",  # Missing eden/scm's commands
            "test-fastreplay-inline-args.t",  # Returns different data in OSS
            "test-gitimport-octopus.t",  # Unknown, fails on GitHub MacOs
            "test-gitimport.t",  # Issue with hggit extension
            "test-hook-tailer.t",  # Issue with hggit extension
            "test-redaction.t",  # This test is temporary broken
            "test-remotefilelog-lfs-client-certs.t",  # Returns different data in OSS
            "test-server.t",  # Returns different data in OSS
            "test-unbundle-replay-hg-recording.t",  # Returns different data in OSS
        },
    }

    if platform == "darwin":
        test_groups[TestGroup.BROKEN].update(
            {"test-pushrebase-block-casefolding.t"}  # MacOS is path case insensitive
        )

    manual_groups = set()
    not_unique = set()
    for group in test_groups.values():
        not_unique.update(manual_groups & group)
        manual_groups.update(group)
    assert not not_unique, f"The test groups contain not unique tests: {not_unique}"

    test_groups[TestGroup.ALL] = all_tests = {
        basename(p)
        for p in iglob(join(repo_root, "eden/mononoke/tests/integration/*.t"))
    }

    not_existing = manual_groups - all_tests
    assert (
        not not_existing
    ), f"The test groups contain not existing tests: {not_existing}"

    rest_groups = all_tests - manual_groups
    # The test-scs* tests use the scs_server which is not buildable in OSS yet
    test_groups[TestGroup.REQUIRING_SCS] = requiring_scs = {
        t for t in rest_groups if t.startswith("test-scs")
    }
    test_groups[TestGroup.PASSING] = rest_groups - requiring_scs

    return test_groups


def get_tests_to_run(repo_root, tests, groups_to_run, rerun_failed):
    test_groups = get_test_groups(repo_root)

    groups_to_run = set(groups_to_run or ([TestGroup.PASSING] if not tests else []))

    tests_to_run = set()
    for group in groups_to_run:
        tests_to_run.update(test_groups[group])

    if tests:
        tests_to_run.update({basename(p) for p in tests or []})

    if rerun_failed:
        # Based on eden/scm/tests/run-tests.py
        for title in ("failed", "errored"):
            failed = Path(repo_root) / "eden/mononoke/tests/integration/.test{}".format(
                title
            )
            if failed.is_file():
                tests_to_run.update(t for t in failed.read_text().splitlines() if t)

    return tests_to_run


def main():
    args = parse_args()
    install_dir = args.install_dir
    build_dir = args.build_dir
    repo_root = dirname(dirname(dirname(dirname(dirname(abspath(__file__))))))

    prepare_manifest_deps(install_dir, build_dir, repo_root)

    tests_to_run = get_tests_to_run(
        repo_root, args.tests, args.test_groups, args.rerun_failed
    )

    env = dict(os.environ.items())
    env["NO_LOCAL_PATHS"] = "1"
    eden_scm_packages = join(install_dir, "eden_scm/lib/python2.7/site-packages")
    pythonpath = env.get("PYTHONPATH")
    env["PYTHONPATH"] = eden_scm_packages + (
        ":{}".format(pythonpath) if pythonpath else ""
    )

    if tests_to_run:
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
                + list(tests_to_run),
                env=env,
            ).returncode
        )


if __name__ == "__main__":
    main()
