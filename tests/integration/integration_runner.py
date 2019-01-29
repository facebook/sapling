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
import sys
import tempfile
import xml.etree.ElementTree as ET

import click

from libfb.py import parutil, pathutils

from .third_party import hg_run_tests

TESTDIR_PATH = 'scm/mononoke/tests/integration'

MONONOKE_BLOBIMPORT_TARGET = '//scm/mononoke:blobimport'
MONONOKE_ALIAS_VERIFY_TARGET = '//scm/mononoke:aliasverify'
MONONOKE_BONSAI_VERIFY_TARGET = '//scm/mononoke:bonsai_verify'
MONONOKE_APISERVER_TARGET = '//scm/mononoke/apiserver:apiserver'
DUMMYSSH_TARGET = '//scm/mononoke/tests/integration:dummyssh'
BINARY_HG_TARGET = '//scm/hg:hg'
MONONOKE_HGCLI_TARGET = '//scm/mononoke/hgcli:hgcli'
MONONOKE_SERVER_TARGET = '//scm/mononoke:mononoke'
FACEBOOK_HOOKS_TARGET = '//scm/mononoke/facebook/hooks:hooks'


@click.command()
@click.option('--dry-run', default=False, is_flag=True, help='list tests')
@click.option(
    '--interactive',
    default=False,
    is_flag=True,
    help='prompt to accept changed output'
)
@click.option('--output', default='', help='output directory')
@click.option(
    '--verbose', default=False, is_flag=True, help='output verbose messages'
)
@click.option(
    '--debug',
    default=False,
    is_flag=True,
    help='debug mode: write output of test scripts to console rather than '
    'capturing and diffing it (disables timeout)'
)
@click.option(
    '--keep-tmpdir',
    default=False,
    is_flag=True,
    help='keep temporary directory after running tests'
)
@click.option(
    '--simple-test-selector',
    default=None,
    help='select an individual test to run',
)
@click.argument(
    'tests',
    nargs=-1,
    type=click.Path(),
)
@click.pass_context
def run(
    ctx, tests, dry_run, interactive, output, verbose, debug,
    simple_test_selector, keep_tmpdir
):
    runner = hg_run_tests.TestRunner()

    testdir = parutil.get_dir_path(TESTDIR_PATH)
    # Also add to the system path because the Mercurial run-tests.py does an
    # absolute import of killdaemons etc.
    sys.path.insert(0, os.path.join(testdir, 'third_party'))

    # Use hg.real to avoid going through the wrapper and incurring slowdown
    # from subprocesses.
    # XXX is this the right thing to do?
    args = ['--with-hg', get_hg_binary()]
    if dry_run:
        args.append('--list-tests')
    if interactive:
        args.append('-i')
    if verbose:
        args.append('--verbose')
    if debug:
        args.append('--debug')
    if keep_tmpdir:
        args.append('--keep-tmpdir')
    args.extend(['-j', '%d' % multiprocessing.cpu_count()])
    if simple_test_selector is not None:
        suite, test = simple_test_selector.split(',', 1)
        if suite != 'run-tests':
            raise click.BadParameter(
                'suite should always be "run-tests"', ctx,
                param_hint='simple_test_selector'
            )
        args.append(test)
    if tests:
        args.extend(tests)

    # In --dry-run mode, the xunit output has to be written to stdout.
    # In regular (run-tests) mode, the output has to be written to the specified
    # output directory.
    if output == '':
        output = None
    _fp, xunit_output = tempfile.mkstemp(dir=output)

    add_to_environ('MONONOKE_BLOBIMPORT', MONONOKE_BLOBIMPORT_TARGET)
    add_to_environ('MONONOKE_ALIAS_VERIFY', MONONOKE_ALIAS_VERIFY_TARGET)
    add_to_environ('MONONOKE_BONSAI_VERIFY', MONONOKE_BONSAI_VERIFY_TARGET)
    add_to_environ(
        'DUMMYSSH', DUMMYSSH_TARGET, pathutils.BuildRuleTypes.PYTHON_BINARY
    )
    add_to_environ('MONONOKE_APISERVER', MONONOKE_APISERVER_TARGET)
    add_to_environ('MONONOKE_HGCLI', MONONOKE_HGCLI_TARGET)
    add_to_environ('MONONOKE_SERVER', MONONOKE_SERVER_TARGET)
    add_to_environ(
        'FACEBOOK_HOOKS',
        FACEBOOK_HOOKS_TARGET,
        pathutils.BuildRuleTypes.FILEGROUP
    )

    # Provide an output directory so that we don't write to a xar's read-only
    # filesystem.
    output_dir = tempfile.mkdtemp()
    try:
        args.extend(['--xunit', xunit_output, '--outputdir', output_dir])
        with contextlib.redirect_stdout(sys.stderr):
            # Do this here to influence as little code as possible -- in
            # particular, add_to_environ depends on getcwd always being inside
            # fbcode
            os.chdir(testdir)
            ret = runner.run(args)

        if dry_run:
            # The output must go to stdout. Set simple_test_selector to make
            # execution runs simpler.
            with open(xunit_output, 'rb') as f:
                xunit_xml = ET.parse(f)
            xunit_xml.getroot().set(
                'runner_capabilities', 'simple_test_selector'
            )
            xunit_xml.write(sys.stdout.buffer, xml_declaration=True)

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
        BINARY_HG_TARGET, pathutils.BuildRuleTypes.PYTHON_BINARY
    )


if __name__ == '__main__':
    run()
