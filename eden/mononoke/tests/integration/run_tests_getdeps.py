#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
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


def script_dir():
    return dirname(abspath(__file__))


def parse_args():
    parser = argparse.ArgumentParser(
        description="Run Mononoke integration tests from getdeps.py build"
    )
    subs = parser.add_subparsers()

    local_parser = subs.add_parser(
        "local",
        help=(
            "Command to run tests from the current checkout of repo assuming that"
            " the required dependencies were already installed by getdeps"
            " (getdeps.py build mononoke_integration)."
        ),
    )
    local_parser.set_defaults(func=local_cmd)

    getdeps_parser = subs.add_parser(
        "getdeps",
        help=(
            "Command that is invoked by getdeps, you probably don't want to call it"
            " directly."
        ),
    )
    getdeps_parser.set_defaults(func=getdeps_cmd)
    getdeps_parser.add_argument(
        "--generate_manifest",
        help="Generate manifest.json file in build directory and don't run tests",
        action="store_true",
    )

    for p in (local_parser, getdeps_parser):
        p.add_argument("getdeps_install_dir", help="Location of getdeps.py install dir")
        p.add_argument(
            "tests",
            nargs="*",
            help="Optional list of tests to run. If provided the --test-groups default is None",
        )
        p.add_argument(
            "-t",
            "--test-groups",
            type=TestGroup,
            nargs="*",
            choices=list(TestGroup),
            help=f"Choose groups of tests to run, default: [{TestGroup.PASSING}]",
        )
        p.add_argument(
            "-r",
            "--rerun-failed",
            action="store_true",
            help="Rerun failed tests based on '.testfailed' file",
        )
        p.add_argument(
            "--dry-run",
            action="store_true",
            help="Just print which tests will be run without running them",
        )
        p.add_argument(
            "--keep-tmpdir",
            action="store_true",
            help="Keep temporary directory after running tests",
        )

    return parser.parse_args()


def local_cmd(args):
    install_dir = args.getdeps_install_dir
    repo_root = dirname(dirname(dirname(dirname(script_dir()))))

    if not args.dry_run:
        prepare_manifest_deps(install_dir, repo_root)
    run_tests(args, join(install_dir, "../build/mononoke_integration"))


def getdeps_cmd(args):
    install_dir = args.getdeps_install_dir
    mononoke_repo_root = join(install_dir, "mononoke/source")

    if args.generate_manifest:
        prepare_manifest_deps(install_dir, mononoke_repo_root)
    else:
        run_tests(args, join(install_dir, "mononoke_integration"))


def prepare_manifest_deps(install_dir, mononoke_repo_root):
    build_dir = join(install_dir, "../build/mononoke_integration")
    manifest_deps_path = join(script_dir(), "manifest_deps")

    exec(
        "global OSS_DEPS; global MONONOKE_BINS; global EDENSCM_BINS; "
        + open(manifest_deps_path, "r").read()
    )

    MANIFEST_DEPS = {}
    for k, v in OSS_DEPS.items():  # noqa: F821
        if v.startswith("//"):
            MANIFEST_DEPS[k] = join(mononoke_repo_root, v[2:])
        elif v.startswith("/"):
            installdep = join(install_dir, v[1:])
            print(f"Adding install dependency {installdep}")
            MANIFEST_DEPS[k] = installdep
        else:
            MANIFEST_DEPS[k] = v
    for k, v in MONONOKE_BINS.items():  # noqa: F821
        MANIFEST_DEPS[k] = join(install_dir, "mononoke/bin", v)

    os.makedirs(build_dir, exist_ok=True)
    with open(join(build_dir, "manifest.json"), "w") as f:
        f.write(json.dumps(MANIFEST_DEPS, sort_keys=True, indent=4))


def get_test_groups():
    test_groups = {
        TestGroup.TIMING_OUT: {
            "test-blobimport-lfs.t",
            "test-infinitepush-lfs.t",
            "test-lfs-copytracing.t",
            "test-lfs-server-acl-check.t",
            "test-lfs-server-consistent-hashing.t",
            "test-lfs-server-disabled-hostname-resolution.t",
            "test-lfs-server-identity-parsing-untrusted.t",
            "test-lfs-server-identity-parsing.t",
            "test-lfs-server-max-upload-size.t",
            "test-lfs-server-proxy-sync.t",
            "test-lfs-server-proxy.t",
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
        TestGroup.FLAKY: {
            "test-cache-warmup-microwave.t",
            "test-gitimport-octopus.t",
            "test-megarepo-fixup-history.t",
            "test-mononoke-hg-sync-job-with-copies.t",
            "test-mononoke-sync-to-backup-readonly.t",
            "test-walker-count-objects.t",
            "test-walker-error-as-data.t",
            # the following might just need expectations updating, not had time to check
            "test-hook-limit-path-length.t",
            "test-hook-no-insecure-filenames.t",
            "test-mononoke-hg-sync-job.t",
            "test-newadmin-blobstore.t",
            "test-newadmin-convert.t",
            "test-newadmin-fetch.t",
            "test-testtool-drawdag.t",
        },
        TestGroup.BROKEN: set(),
    }

    if platform == "darwin":
        test_groups[TestGroup.BROKEN].update(
            {
                "test-pushrebase-block-public-casefolding.t",  # MacOS is case insensitive
            }
        )

    manual_groups = set()
    not_unique = set()
    for group in test_groups.values():
        not_unique.update(manual_groups & group)
        manual_groups.update(group)
    assert not not_unique, f"The test groups contain not unique tests: {not_unique}"

    test_groups[TestGroup.ALL] = all_tests = {
        basename(p) for p in iglob(join(script_dir(), "*.t"))
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


def get_tests_to_run(tests, groups_to_run, rerun_failed, test_root_public):
    test_groups = get_test_groups()

    groups_to_run = set(
        groups_to_run or ([TestGroup.PASSING] if not (tests or rerun_failed) else [])
    )

    tests_to_run = set()
    for group in groups_to_run:
        tests_to_run.update(test_groups[group])

    if tests:
        tests_to_run.update({basename(p) for p in tests or []})

    if rerun_failed:
        # Based on eden/scm/tests/run-tests.py
        for title in ("failed", "errored"):
            failed = Path(test_root_public) / ".test{}".format(title)
            if failed.is_file():
                tests_to_run.update(t for t in failed.read_text().splitlines() if t)

    return tests_to_run


def get_pythonpath(getdeps_install_dir):
    paths = [join(getdeps_install_dir, "eden_scm/lib/python2.7/site-packages")]

    _, installed, _ = next(os.walk(getdeps_install_dir))

    packages = ["click", "dulwich"]

    for package in packages:
        candidates = [i for i in installed if i.startswith(f"python-{package}-")]
        if len(candidates) == 0:
            raise Exception(
                f"Failed to find 'python-{package}' in installed directory,"
                " did you run getdeps?"
            )
        if len(candidates) > 1:
            raise Exception(
                f"Found more than one 'python-{package}' package in installed"
                "directory, try cleaning the install dir and rerunning getdeps"
            )
        paths.append(
            join(getdeps_install_dir, candidates[0], f"lib/fb-py-libs/python-{package}")
        )

    pythonpath = os.environ.get("PYTHONPATH")
    return ":".join(paths) + (":{}".format(pythonpath) if pythonpath else "")


def run_tests(args, manifest_json_dir):
    manifest_json_path = join(manifest_json_dir, "manifest.json")
    with open(manifest_json_path) as json_file:
        manifest = json.load(json_file)
        test_root_public = manifest["TEST_ROOT_PUBLIC"]

    tests_to_run = get_tests_to_run(
        args.tests, args.test_groups, args.rerun_failed, test_root_public
    )

    env = dict(os.environ.items())
    env["NO_LOCAL_PATHS"] = "1"
    env["PYTHONPATH"] = get_pythonpath(args.getdeps_install_dir)

    if args.dry_run:
        print("\n".join(tests_to_run))
    elif tests_to_run:
        cmd = [
            sys.executable,
            join(script_dir(), "integration_runner_real.py"),
            manifest_json_path,
        ]
        if args.keep_tmpdir:
            cmd.append("--keep-tmpdir")
        sys.exit(subprocess.run(cmd + sorted(list(tests_to_run)), env=env).returncode)


if __name__ == "__main__":
    args = parse_args()
    args.func(args)
