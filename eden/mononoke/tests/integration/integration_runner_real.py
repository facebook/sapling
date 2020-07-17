#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Runner for Mononoke/Mercurial integration tests."""

import json
import logging
import multiprocessing
import os
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from typing import Any, Dict, List, NamedTuple, Optional, Set

import click


ManifestEnv = Dict[str, str]
Args = List[str]
Env = Dict[str, str]


SUITE = "run-tests"

EPHEMERAL_DB_ALLOWLIST = {
    "test-init.t",
    "test-lookup.t",
    "test-mononoke-admin.t",
    "test-bookmarks-filler.t",
    "test-pushrebase.t",
    "test-mononoke-hg-sync-job-generate-bundles-loop.t",
    "test-blobstore-healer.t",
    "test-infinitepush-mutation.t",
}

# At this time, all tests support the network void script (except when
# ephemeral MySQL is used)
DISABLE_ALL_NETWORK_ACCESS_SKIPLIST: Set[str] = {
    "test-commitcloud-forwardfiller.t",
    "test-commitcloud-reversefiller.t",
}


def is_mode_opt_buck_binary():
    try:
        import __manifest__

        return __manifest__.fbmake["build_mode"] == "opt"
    except ImportError:
        return False


def is_libfb_present():
    try:
        import libfb.py.log  # noqa: F401

        return True
    except ImportError:
        return False


class TestFlags(NamedTuple):
    interactive: bool
    verbose: bool
    debug: bool
    keep_tmpdir: bool
    disable_all_network_access: bool

    def runner_args(self) -> Args:
        r = []

        if self.interactive:
            r.append("--interactive")

        if self.verbose:
            r.append("--verbose")

        if self.debug:
            r.append("--debug")

        if self.keep_tmpdir:
            r.append("--keep-tmpdir")

        r.extend(["-j", "%d" % multiprocessing.cpu_count()])

        return r

    def runner_kwargs(self, tests: List[str]) -> Dict[str, bool]:
        r = {}

        if self.interactive:
            r["interactive"] = True

        if self.disable_all_network_access:
            incompatible_tests = set(tests) & set(DISABLE_ALL_NETWORK_ACCESS_SKIPLIST)
            if incompatible_tests:
                logging.warning(
                    "Not enabling network void because incompatible "
                    "tests are to be run: %s",
                    " ".join(incompatible_tests),
                )
            else:
                r["disable_all_network_access"] = True

        return r


def public_test_root(manifest_env: ManifestEnv) -> str:
    return manifest_env["TEST_ROOT_PUBLIC"]


def facebook_test_root(manifest_env: ManifestEnv) -> Optional[str]:
    return manifest_env.get("TEST_ROOT_FACEBOOK")


def maybe_use_local_test_paths(manifest_env: ManifestEnv):
    # If we are running outside of Buck, then update the test paths to use the
    # actual files. This makes --interactive work, and allows for adding new
    # tests, etc., without rebuilding the runner.
    if int(os.environ.get("NO_LOCAL_PATHS", 0)):
        return

    fbsource = subprocess.check_output(["hg", "root"], encoding="utf-8").strip()
    fbcode = os.path.join(fbsource, "fbcode")
    tests = os.path.join(fbcode, "eden/mononoke/tests/integration")

    manifest_env.update(
        {
            "TEST_CERTS": os.path.join(tests, "certs/facebook"),
            "TEST_ROOT_PUBLIC": tests,
            "TEST_ROOT_FACEBOOK": os.path.join(tests, "facebook"),
            "TEST_FIXTURES": tests,
            "RUN_TESTS_LIBRARY": os.path.join(fbcode, "eden/scm/tests"),
        }
    )


def _hg_runner(
    root,
    manifest_env: ManifestEnv,
    extra_args: Args,
    extra_env: Env,
    disable_all_network_access: bool = False,
    interactive: bool = False,
    quiet: bool = False,
):
    if "SANDCASTLE" in os.environ:
        # Sandcastle's /tmp might be mounted on a slow device
        # In that case let's move the test tmp dir to /dev/shm
        os.environ["TMPDIR"] = "/dev/shm"

    with tempfile.TemporaryDirectory() as output_dir:
        args = [
            manifest_env["BINARY_HGPYTHON"],
            os.path.join(manifest_env["RUN_TESTS_LIBRARY"], "run-tests.py"),
            "--maxdifflines=1000",
            "--with-hg",
            manifest_env["BINARY_HG"],
            "--chg",
            "--outputdir",
            output_dir,
            *extra_args,
        ]

        # The network void script breaks opt mode PAR binaries, so that
        # breaks hg's run tests (that is because /tmp has the wrong owner,
        # because we are running in a user namespace), so we don't enable it if
        # we're running in opt. This is necessary ...  but it's also kinda meh
        # for 2 reasons:
        # - It means mode/opt tests can talk to the network. This is probably
        # fine since running in mode/dev would probably catch problems first if
        # any.
        # - The fact that this binary was built in mode/opt does not
        # necessarily mean that the hg run tests binary was. It's a decent
        # approximation, though.
        if disable_all_network_access and not is_mode_opt_buck_binary():
            args.insert(0, manifest_env["DISABLE_ALL_NETWORK_ACCESS"])

        env = os.environ.copy()
        env.update(
            {
                "HGPYTHONPATH": manifest_env["RUN_TESTS_LIBRARY"],
                "PYTHON_SYS_EXECUTABLE": manifest_env["BINARY_HGPYTHON"],
            }
        )
        env.update(manifest_env)
        env.update(extra_env)

        stdin = None
        stderr: Any = subprocess.DEVNULL if quiet else sys.stderr.buffer

        subprocess.check_call(
            args, cwd=root, env=env, stdin=stdin, stdout=stderr, stderr=stderr
        )


def hg_runner_public(manifest_env: Env, *args, **kwargs):
    _hg_runner(public_test_root(manifest_env), manifest_env, *args, **kwargs)
    return True


def hg_runner_facebook(manifest_env: Env, *args, **kwargs):
    fb_root = facebook_test_root(manifest_env)
    if fb_root is None:
        return False
    else:
        _hg_runner(fb_root, manifest_env, *args, **kwargs)
        return True


def discover_tests(manifest_env: Env, mysql: bool):
    all_tests = []

    for runner in [hg_runner_public, hg_runner_facebook]:
        with tempfile.NamedTemporaryFile(mode="rb") as f:
            if runner(
                manifest_env, ["--list-tests", "--xunit", f.name], {}, quiet=True
            ):
                xml = ET.parse(f)
                suite = xml.getroot()
                for child in suite:
                    all_tests.append(child.get("name"))

    if mysql:
        all_tests = [t for t in all_tests if t in EPHEMERAL_DB_ALLOWLIST]

    return all_tests


def run_discover_tests(
    ctx: Any, manifest_env: ManifestEnv, xunit_output: str, mysql: bool
):
    tests = discover_tests(manifest_env, mysql)

    if xunit_output is None:
        print("\n".join(tests))
        return

    root = ET.Element(
        "testsuite", {"name": SUITE, "runner_capabilities": "simple_test_selector"}
    )
    root.extend([ET.Element("testcase", {"name": t}) for t in tests])

    if not os.path.exists(xunit_output):
        os.makedirs(xunit_output)

    with open(os.path.join(xunit_output, "suite.xml"), "wb") as f:
        ET.ElementTree(root).write(f, xml_declaration=True)

    ctx.exit(0)


def run_tests(
    ctx: Any,
    manifest_env: ManifestEnv,
    xunit_output: str,
    tests: List[str],
    test_flags: TestFlags,
    test_env: Env,
):
    # If junit output has been requested, then that means testpilot is calling
    # us, and if testpilot is calling us, we shouldn't be running more than one
    # test at a time. Since we don't want to try and have to do anything smart
    # about coalescing results from reguler tests and FB tests, we just make
    # this an error.
    if xunit_output is not None and len(tests) > 1:
        raise click.BadParameter("Cannot run more than one test with --output", ctx)

    public_tests = []
    facebook_tests = []
    missing_tests = []

    for t in tests:
        fb_root = facebook_test_root(manifest_env)
        if os.path.isfile(os.path.join(public_test_root(manifest_env), t)):
            public_tests.append(t)
        elif fb_root is not None and os.path.isfile(os.path.join(fb_root, t)):
            facebook_tests.append(t)
        else:
            missing_tests.append(t)

    if missing_tests:
        raise click.BadParameter("Invalid tests: %s" % " ".join(missing_tests), ctx)

    work = []
    if public_tests:
        work.append(("public", hg_runner_public, public_tests))
    if facebook_tests:
        work.append(("facebook", hg_runner_facebook, facebook_tests))

    success = True

    for (prefix, runner, tests) in work:
        args = list(test_flags.runner_args())

        if xunit_output is not None:
            xunit_file = os.path.join(xunit_output, ".".join([prefix, "xml"]))
            args.extend(["--xunit", xunit_file])
        args.extend(tests)

        try:
            kwargs = test_flags.runner_kwargs(tests)
            runner(manifest_env, args, test_env, **kwargs)
        except subprocess.CalledProcessError:
            success = False

    ctx.exit(0 if success else 1)


@click.command()
@click.option("--dry-run", default=False, is_flag=True, help="list tests")
@click.option(
    "--interactive", default=False, is_flag=True, help="prompt to accept changed output"
)
@click.option("--output", default=None, help="output directory")
@click.option("--verbose", default=False, is_flag=True, help="output verbose messages")
@click.option(
    "--debug",
    default=False,
    is_flag=True,
    help="debug mode: write output of test scripts to console rather than "
    "capturing and diffing it (disables timeout)",
)
@click.option(
    "--keep-tmpdir",
    default=False,
    is_flag=True,
    help="keep temporary directory after running tests",
)
@click.option(
    "--simple-test-selector", default=None, help="select an individual test to run"
)
@click.option(
    "--mysql",
    default=False,
    is_flag=True,
    help="Use Ephemeral DB or optionally devdb to run tests with MySQL",
)
@click.option(
    "mysql_schemas",
    "--mysql-schema",
    multiple=True,
    help="Import schema into mysql Ephemeral DB or devdb (Configerator path)",
)
@click.option(
    "--devdb",
    default=None,
    is_flag=False,
    help="Use devdb to run tests with MySQL, specify the shard e.g. --devdb=$USER",
)
@click.argument("manifest", type=click.Path())
@click.argument("tests", nargs=-1, type=click.Path())
@click.pass_context
def run(
    ctx,
    manifest,
    tests,
    dry_run,
    interactive,
    output,
    verbose,
    debug,
    simple_test_selector,
    keep_tmpdir,
    mysql,
    mysql_schemas,
    devdb,
):
    manifest = os.path.abspath(manifest)
    if is_libfb_present():
        from libfb.py.log import set_simple_logging
        from eden.mononoke.tests.integration.facebook.lib_runner import (
            load_manifest_env,
        )

        set_simple_logging(logging.INFO)
        manifest_env: ManifestEnv = load_manifest_env(manifest)
    else:
        with open(manifest) as f:
            manifest_env: ManifestEnv = json.load(f)

    maybe_use_local_test_paths(manifest_env)

    if dry_run:
        return run_discover_tests(ctx, manifest_env, output, mysql)

    test_flags: TestFlags = TestFlags(
        interactive,
        verbose,
        debug,
        keep_tmpdir,
        disable_all_network_access=(
            not mysql and "DISABLE_ALL_NETWORK_ACCESS" in manifest_env
        ),  # NOTE: We need network to talk to MySQL
    )

    selected_tests: List[str] = []
    if simple_test_selector is not None and tests:
        raise click.BadParameter(
            "Use either --simple-test-selector, or [...TESTS]", ctx
        )
    elif simple_test_selector is not None:
        suite, test = simple_test_selector.split(",", 1)
        if suite != "run-tests":
            raise click.BadParameter(
                'suite should always be "%s"' % SUITE,
                ctx,
                param_hint="simple_test_selector",
            )
        selected_tests.append(test)
    else:
        selected_tests.extend(tests)

    if is_libfb_present():
        from eden.mononoke.tests.integration.facebook.lib_runner import fb_test_context

        with fb_test_context(
            ctx, dry_run, mysql, mysql_schemas, devdb, selected_tests
        ) as test_env:
            run_tests(ctx, manifest_env, output, selected_tests, test_flags, test_env)
    else:
        run_tests(ctx, manifest_env, output, selected_tests, test_flags, test_env={})


if __name__ == "__main__":
    run()
