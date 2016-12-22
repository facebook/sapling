#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

'''
These utilities are only expected to work if `sys.argv[0]` is an executable
being run in buck-out.
'''

import os
import sys


def _find_directories():
    '''Returns the paths to buck-out and the repo root.

    Note that the path to buck-out may not be "buck-out" under the repo root
    because Buck could have been run with `buck --config project.buck_out` and
    sys.argv[0] could be the realpath rather than the symlink under buck-out.

    TODO: We will have to use a different heuristic for open source builds that
    build with CMake. (Ultimately, we would prefer to build them with Buck.)
    '''
    executable = sys.argv[0]
    path = os.path.dirname(os.path.abspath(executable))
    while True:
        parent = os.path.dirname(path)
        parent_basename = os.path.basename(parent)
        if parent_basename == 'buck-out':
            repo_root = os.path.dirname(parent)
            if os.path.basename(path) in ['bin', 'gen']:
                buck_out = parent
            else:
                buck_out = path
            return repo_root, buck_out
        if parent == path:
            raise Exception('Path to repo root not found from %s' % executable)
        path = parent


REPO_ROOT, BUCK_OUT = _find_directories()

# The EDENFS_SUFFIX will be set to indicate if we should test with a
# particular variant of the edenfs daemon
EDENFS_SUFFIX = os.environ.get('EDENFS_SUFFIX', '')


def _find_cli():
    cli = os.environ.get('EDENFS_CLI_PATH')
    if not cli:
        cli = os.path.join(BUCK_OUT, 'gen/eden/fs/cli/cli.par')
    if not os.access(cli, os.X_OK):
        msg = 'unable to find eden CLI for integration testing: {!r}'.format(
            cli)
        raise Exception(msg)
    return cli


EDEN_CLI = _find_cli()


def _find_daemon():
    edenfs = os.environ.get('EDENFS_SERVER_PATH')
    if not edenfs:
        edenfs = os.path.join(BUCK_OUT, 'gen/eden/fs/service/edenfs%s' % EDENFS_SUFFIX)
    if not os.access(edenfs, os.X_OK):
        msg = 'unable to find eden daemon for integration testing: {!r}'.format(
            edenfs)
        raise Exception(msg)
    return edenfs


EDEN_DAEMON = _find_daemon()
