#!/usr/bin/env python3
# Copyright (c) 2004-present, Facebook, Inc.
# All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Runner for Mononoke/Mercurial integration tests."""

import contextlib
import multiprocessing
import os
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

import click

from common.db.tests import DbDef
from libfb.py import parutil, pathutils


@contextlib.contextmanager
def ephemeral_db_helper():
    with DbDef.TestDatabaseManager(
        prefix="mononoke_tests",
        oncall_shortname="source_control",
        force_ephemeral=True,
        source_shard="xdb.mononoke_production",
        ttl_minutes=15,  # Some Mononoke tests last this long
    ) as test_db_manager:
        yield {"DB_SHARD_NAME": test_db_manager.get_shard_name()}


@contextlib.contextmanager
def no_db_helper():
    yield {}


EPHEMERAL_DB = "USE_EPHEMERAL_DB"
EPHEMERAL_DB_WHITELIST = {
    "test-init.t",
    "test-lookup.t",
    "test-mononoke-admin.t",
}

TESTDIR_PATH = "scm/mononoke/tests/integration"

MONONOKE_BLOBIMPORT_TARGET = "//scm/mononoke:blobimport"
MONONOKE_ADMIN_TARGET = "//scm/mononoke:admin"
MONONOKE_ALIAS_VERIFY_TARGET = "//scm/mononoke:aliasverify"
MONONOKE_BONSAI_VERIFY_TARGET = "//scm/mononoke:bonsai_verify"
MONONOKE_APISERVER_TARGET = "//scm/mononoke/apiserver:apiserver"
DUMMYSSH_TARGET = "//scm/mononoke/tests/integration:dummyssh"
INTEGRATION_SHELL_TARGET = "//scm/mononoke/tests/integration:integration_shell"
BINARY_HG_TARGET = "//scm/hg:hg"
BINARY_HGPYTHON_TARGET = "//scm/hg:hgpython"
MONONOKE_HGCLI_TARGET = "//scm/mononoke/hgcli:hgcli"
MONONOKE_SERVER_TARGET = "//scm/mononoke:mononoke"
FACEBOOK_HOOKS_TARGET = "//scm/mononoke/facebook/hooks:hooks"
PUSHREBASE_REPLAY_TARGET = "//scm/mononoke/facebook/pushrebase_replay:pushrebase_replay"
VERIFY_INTEGRITY_TARGET = "//security/source_control:verify_integrity"
MONONOKE_HG_SYNC_TARGET = (
    "//scm/mononoke/facebook/mononoke_hg_sync_job:mononoke_hg_sync_job"
)
WRITE_STUB_LOG_ENTRY_TARGET = (
    "//scm/mononoke/tests/write_stub_log_entry:write_stub_log_entry"
)


@click.command()
@click.option("--dry-run", default=False, is_flag=True, help="list tests")
@click.option(
    "--interactive", default=False, is_flag=True, help="prompt to accept changed output"
)
@click.option("--output", default="", help="output directory")
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
@click.argument("tests", nargs=-1, type=click.Path())
@click.pass_context
def run(
    ctx,
    tests,
    dry_run,
    interactive,
    output,
    verbose,
    debug,
    simple_test_selector,
    keep_tmpdir,
):
    db_helper = no_db_helper

    testdir = parutil.get_dir_path(TESTDIR_PATH)
    run_tests_dir = os.path.join(
        os.path.join(testdir, "third_party"), "hg_run_tests.py"
    )
    args = [
        get_hg_python_binary(),
        run_tests_dir,
        "--maxdifflines=1000",
        "--with-hg",
        get_hg_binary(),
    ]
    if dry_run:
        args.append("--list-tests")
    if interactive:
        args.append("-i")
    if verbose:
        args.append("--verbose")
    if debug:
        args.append("--debug")
    if keep_tmpdir:
        args.append("--keep-tmpdir")
    args.extend(["-j", "%d" % multiprocessing.cpu_count()])

    matched_tests = []

    if simple_test_selector is not None:
        suite, test = simple_test_selector.split(",", 1)
        if suite != "run-tests":
            raise click.BadParameter(
                'suite should always be "run-tests"',
                ctx,
                param_hint="simple_test_selector",
            )
        matched_tests.append(test)

    if tests:
        matched_tests.extend(tests)

    use_ephemeral_db = int(os.environ.get(EPHEMERAL_DB, 0))

    if use_ephemeral_db:
        if dry_run:
            # We can dry-run for ephemeral tests
            pass
        elif len(matched_tests) <= 1:
            # One test is allowed to use an ephemeral db. This is OK, because
            # TestPilot actually asks us to run tests one by one.
            db_helper = ephemeral_db_helper
        else:
            # Too many tests! This won't work.
            raise click.BadParameter(
                "%s allows at most 1 test at a time" % EPHEMERAL_DB,
                ctx,
                param_hint="simple_test_selector"
            )

    args.extend(matched_tests)

    # In --dry-run mode, the xunit output has to be written to stdout.
    # In regular (run-tests) mode, the output has to be written to the specified
    # output directory.
    if output == "":
        output = None
    _fp, xunit_output = tempfile.mkstemp(dir=output)

    add_to_environ("MONONOKE_BLOBIMPORT", MONONOKE_BLOBIMPORT_TARGET)
    add_to_environ("MONONOKE_ALIAS_VERIFY", MONONOKE_ALIAS_VERIFY_TARGET)
    add_to_environ("MONONOKE_ADMIN", MONONOKE_ADMIN_TARGET)
    add_to_environ("MONONOKE_BONSAI_VERIFY", MONONOKE_BONSAI_VERIFY_TARGET)
    add_to_environ("DUMMYSSH", DUMMYSSH_TARGET, pathutils.BuildRuleTypes.PYTHON_BINARY)
    add_to_environ("MONONOKE_APISERVER", MONONOKE_APISERVER_TARGET)
    add_to_environ("MONONOKE_HGCLI", MONONOKE_HGCLI_TARGET)
    add_to_environ("MONONOKE_SERVER", MONONOKE_SERVER_TARGET)
    add_to_environ(
        "FACEBOOK_HOOKS", FACEBOOK_HOOKS_TARGET, pathutils.BuildRuleTypes.FILEGROUP
    )
    add_to_environ("MONONOKE_HG_SYNC", MONONOKE_HG_SYNC_TARGET)
    add_to_environ("WRITE_STUB_LOG_ENTRY", WRITE_STUB_LOG_ENTRY_TARGET)
    add_to_environ(
        "PUSHREBASE_REPLAY",
        PUSHREBASE_REPLAY_TARGET,
        pathutils.BuildRuleTypes.PYTHON_BINARY,
    )
    add_to_environ(
        "VERIFY_INTEGRITY_TARGET",
        VERIFY_INTEGRITY_TARGET,
        pathutils.BuildRuleTypes.PYTHON_BINARY,
    )

    # Provide an output directory so that we don't write to a xar's read-only
    # filesystem.
    output_dir = tempfile.mkdtemp()
    try:
        args.extend(["--xunit", xunit_output, "--outputdir", output_dir])
        with contextlib.redirect_stdout(sys.stderr):
            # Do this here to influence as little code as possible -- in
            # particular, add_to_environ depends on getcwd always being inside
            # fbcode
            os.chdir(testdir)
            # Also add to the system path because the Mercurial run-tests.py does an
            # absolute import of killdaemons etc.
            env = os.environ.copy()
            env["HGPYTHONPATH"] = os.path.join(testdir, "third_party")

            with db_helper() as db_helper_env:
                env.update(db_helper_env)
                p = subprocess.Popen(
                    args,
                    env=env,
                    stderr=sys.stderr,
                    stdout=sys.stdout
                )
                p.communicate("")
                ret = p.returncode

        # Expose the simple_test_selector capability to TestPilot.
        with open(xunit_output, "rb") as f:
            xunit_xml = ET.parse(f)

        suite = xunit_xml.getroot()
        suite.set("runner_capabilities", "simple_test_selector")

        # Filter our non-whitelisted tests if ephemeraldb is enabled.
        if dry_run and use_ephemeral_db:
            for child in list(suite):
                if child.get("name") in EPHEMERAL_DB_WHITELIST:
                    continue
                suite.remove(child)

        with open(xunit_output, "wb") as f:
            xunit_xml.write(f, xml_declaration=True)

        ctx.exit(ret)
    finally:
        try:
            # If an output was specified, xunit_output is owned by the caller
            # and is the caller's responsibility to clean up.
            if output is None:
                os.unlink(xunit_output)
        except OSError:
            pass
        shutil.rmtree(output_dir, ignore_errors=True)


def add_to_environ(var, target, rule_type=pathutils.BuildRuleTypes.RUST_BINARY):
    os.environ[var] = pathutils.get_build_rule_output_path(target, rule_type)


def get_hg_binary():
    return pathutils.get_build_rule_output_path(
        BINARY_HG_TARGET,
        pathutils.BuildRuleTypes.GENRULE,
        output_name='hg/hg'
    )


def get_hg_python_binary():
    return pathutils.get_build_rule_output_path(
        BINARY_HGPYTHON_TARGET, pathutils.BuildRuleTypes.PYTHON_BINARY
    )


if __name__ == "__main__":
    run()
