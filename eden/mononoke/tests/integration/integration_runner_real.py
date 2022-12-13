#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
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

# At this time, all tests support the network void script (except when
# ephemeral MySQL is used)
ALLOW_NETWORK_ACCESS_BY_NAME: Set[str] = {
    # Purposely not disabling network as this tests reverse dns lookups
    "test-metadata-fb-host.t",
    "test-metadata.t",
    # Purposely not disabling network as this needs to make TLS connections.
    "test-cat-auth.t",
    "test-hook-verify-integrity.t",
    "test-bypass-readonly-acl.t",
    # Components linked with SM library invoke some network calls to loopback
    # address for configerator and ODS even without being part of actual execution.
    # Skip the below tests until RCA is completed for SM integration so
    # that integration tests do not time-out due to failed network call retries.
    # TODO(rajshar): Investigate root cause for network calls from SM Client.
    # Post: https://fb.workplace.com/groups/sm.users/permalink/2490367831097595/
    "test-mirror-hg-commits-basic.t",
    "test-backfill-derived-data.t",
    "test-mononoke-hg-sync-job.t",
    # Tests implicilitly relying on crypto cat library which needs network access to
    # read its configs.
}

# Used for allowing access to many tests at once
ALLOW_NETWORK_ACCESS_BY_PREFIX: Set[str] = {
    "test-scs",
    "test-land-service",
    "test-snapshot",
    "test-remote-gitimport",
    "test-new-walker",
    "test-derived-data-service",
}


def is_libfb_present() -> bool:
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
    tmpdir: Optional[str]
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

        if self.tmpdir:
            r.extend(["--tmpdir", self.tmpdir])

        r.extend(["-j", "%d" % multiprocessing.cpu_count()])

        return r

    def runner_kwargs(self, tests: List[str]) -> Dict[str, bool]:
        r = {}

        if self.interactive:
            r["interactive"] = True

        if self.disable_all_network_access:
            incompatible_tests = set(tests) & set(ALLOW_NETWORK_ACCESS_BY_NAME)
            for test in tests:
                for prefix in ALLOW_NETWORK_ACCESS_BY_PREFIX:
                    if test.startswith(prefix):
                        incompatible_tests.add(test)
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


def maybe_use_local_test_paths(manifest_env: ManifestEnv) -> None:
    # If we are running outside of Buck, then update the test paths to use the
    # actual files. This makes --interactive work, and allows for adding new
    # tests, etc., without rebuilding the runner.
    if int(os.environ.get("NO_LOCAL_PATHS", 0)):
        return

    is_oss_build = facebook_test_root(manifest_env) is None

    fbsource = subprocess.check_output(["hg", "root"], encoding="utf-8").strip()
    fbcode = os.path.join(fbsource, "fbcode")
    tests = os.path.join(
        fbcode,
        manifest_env.get("VERBATIM_LOCAL_PATH", "eden/mononoke/tests/integration"),
    )
    fixtures = os.path.join(fbcode, "eden/mononoke/tests/integration")

    updates_to_apply = {
        "TEST_ROOT_PUBLIC": tests,
        "TEST_FIXTURES": fixtures,
        "RUN_TESTS_LIBRARY": os.path.join(fbcode, "eden/scm/tests"),
    }

    if is_oss_build:
        updates_to_apply["TEST_CERTS"] = os.path.join(fixtures, "certs")
    else:
        updates_to_apply["TEST_CERTS"] = os.path.join(fixtures, "certs/facebook")
        updates_to_apply["TEST_ROOT_FACEBOOK"] = os.path.join(tests, "facebook")

    manifest_env.update(updates_to_apply)


def _hg_runner(
    root: str,
    manifest_env: ManifestEnv,
    extra_args: Args,
    extra_env: Env,
    disable_all_network_access: bool = False,
    chg: bool = False,
    interactive: bool = False,
    quiet: bool = False,
) -> None:
    if "SANDCASTLE" in os.environ and os.path.exists("/dev/shm"):
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
            "--allow-slow-tests",
            "--outputdir",
            output_dir,
            # The eden/scm/test/features.py lists tests by name and doesn't
            # make the distinction between eden/scm and eden/mononoke tests.
            # Disabling this will prevent from accidental enabling of features
            # for Mononoke tests.
            "--nofeatures",
            *extra_args,
        ]

        if chg:
            args.append("--chg")

        if disable_all_network_access:
            args.insert(0, manifest_env["DISABLE_ALL_NETWORK_ACCESS"])

        env = os.environ.copy()
        env.update(
            {
                "HGPYTHONPATH": manifest_env["RUN_TESTS_LIBRARY"],
                # Use native byte values when sorting or manipulating data.
                # Notice that the en_US.UTF-8 locale doesn't behave the same on
                # all systems and trying to run commands like "sed" or "tr" on
                # non-utf8 data will result in "Illegal byte sequence" error.
                # That is why we are forcing the "C" locale.
                "HGTEST_LOCALE": "C",
                "PYTHON_SYS_EXECUTABLE": manifest_env["BINARY_HGPYTHON"],
            }
        )
        env.update(manifest_env)
        env.update(extra_env)

        stdin = None
        stderr: Any = subprocess.DEVNULL if quiet else sys.stderr.buffer

        subprocess.check_call(
            args,
            cwd=root,
            env=env,
            stdin=stdin,
            stdout=stderr,
            stderr=stderr,
        )


def hg_runner_public(manifest_env: Env, *args, **kwargs) -> bool:
    _hg_runner(public_test_root(manifest_env), manifest_env, *args, **kwargs)
    return True


def hg_runner_facebook(manifest_env: Env, *args, **kwargs) -> bool:
    fb_root = facebook_test_root(manifest_env)
    if fb_root is None:
        return False
    else:
        _hg_runner(fb_root, manifest_env, *args, **kwargs)
        return True


def format_discovered_tests(
    tests: List[str],
    ctx: Any,
    xunit_output: str,
) -> None:
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
) -> None:
    # If junit output has been requested, then that means testpilot is calling
    # us, and if testpilot is calling us, we shouldn't be running more than one
    # test at a time. Since we don't want to try and have to do anything smart
    # about coalescing results from regular tests and FB tests, we just make
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
    "capturing and diffing it, and disable network void and chg to ease debugging "
    "(disables timeout)",
)
@click.option(
    "--keep-tmpdir",
    default=False,
    is_flag=True,
    help="keep temporary directory after running tests",
)
@click.option(
    "--tmpdir",
    default=None,
    is_flag=False,
    help="run tests in the given temporary directory (implies --keep-tmpdir)",
)
@click.option(
    "--simple-test-selector", default=None, help="select an individual test to run"
)
@click.option(
    "--mysql-client",
    default=False,
    is_flag=True,
    help="Use Ephemeral DB or optionally devdb to run tests with MySQL using MySQL client",
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
@click.option(
    "discovered_tests",
    "--discovered-test",
    multiple=True,
    help="Tests that are already known to exist",
    type=click.Path(),
)
@click.argument("manifest", type=click.Path())
@click.argument("tests", nargs=-1, type=click.Path())
@click.pass_context
def run(
    ctx,
    manifest,
    tests,
    dry_run: bool,
    interactive,
    output: str,
    verbose,
    debug,
    simple_test_selector,
    keep_tmpdir,
    tmpdir,
    mysql_client: bool,
    mysql_schemas,
    devdb: str,
    discovered_tests,
) -> None:
    manifest = os.path.abspath(manifest)
    if is_libfb_present():
        from eden.mononoke.tests.integration.facebook.lib_buck import find_buck_out
        from libfb.py.log import set_simple_logging

        def load_manifest_env(manifest_path: str) -> Dict[str, str]:
            buck_out = find_buck_out(manifest_path)

            with open(manifest_path) as f:
                manifest_env = json.load(f)

            def process_value(key: str, value: str, buck_out: str) -> str:
                if key.startswith("VERBATIM_"):
                    return value
                return os.path.join(buck_out, value)

            return {k: process_value(k, v, buck_out) for k, v in manifest_env.items()}

        set_simple_logging(logging.INFO)
        manifest_env: ManifestEnv = load_manifest_env(manifest)
    else:
        with open(manifest) as f:
            manifest_env: ManifestEnv = json.load(f)

    maybe_use_local_test_paths(manifest_env)

    if dry_run and discovered_tests:
        return format_discovered_tests(list(discovered_tests), ctx, output)

    test_flags: TestFlags = TestFlags(
        interactive,
        verbose,
        debug,
        keep_tmpdir,
        tmpdir,
        disable_all_network_access=(
            # NOTE: We need network to talk to MySQL
            not debug
            and not mysql_client
            and "DISABLE_ALL_NETWORK_ACCESS" in manifest_env
        ),
    )

    selected_tests: List[str] = []
    if simple_test_selector is not None and tests:
        raise click.BadParameter(
            f"Found --simple-test-selector {simple_test_selector}. Use either --simple-test-selector, or [...TESTS]",
            ctx,
        )
    elif simple_test_selector is not None:
        suite, test = simple_test_selector.split(",", 1)
        if suite != SUITE:
            raise click.BadParameter(
                'suite should always be "%s"' % SUITE,
                ctx,
                param_hint="simple_test_selector",
            )
        selected_tests.append(test)
    else:
        selected_tests.extend(tests)

    try:
        from eden.mononoke.tests.integration.facebook.lib_runner import fb_test_context
    except ImportError:
        run_tests(ctx, manifest_env, output, selected_tests, test_flags, test_env={})
    else:
        with fb_test_context(
            ctx, dry_run, mysql_client, mysql_schemas, devdb, selected_tests
        ) as test_env:
            run_tests(ctx, manifest_env, output, selected_tests, test_flags, test_env)


if __name__ == "__main__":
    run()
