#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

'''
This script populates the .hg directory in a new eden client.

This needs to be run the first time an eden client is mounted so that hg
commands will work properly inside the eden client.
'''
import argparse
import errno
import os
import shutil
import sys
import tempfile


def read_config(directory, name, default=None):
    try:
        with open(os.path.join(directory, name), 'r') as f:
            return f.read()
    except EnvironmentError as ex:
        if ex.errno == errno.ENOENT:
            return default
        else:
            raise


def setup_eden_hg_dir(eden_hg_dir, repo_hg_dir, eden_ext_path):
    if eden_ext_path is None:
        eden_ext_path = ''

    # Set up the hgrc file.
    # Take the settings from the original repository, and add settings required
    # by eden.
    hgrc_data = read_config(repo_hg_dir, 'hgrc')

    # TODO: It would probably be nicer to just append an %include pointing to a
    # file with the extra settings we need.
    extra_hgrc_settings = '''\
# Extension settings required by eden
[extensions]
share =
eden = {}
sqldirstate = !
fsmonitor = !
sparse = !
'''.format(eden_ext_path)

    if not hgrc_data:
        hgrc_data = extra_hgrc_settings
    else:
        hgrc_data = hgrc_data + '\n' + extra_hgrc_settings
    with open(os.path.join(eden_hg_dir, 'hgrc'), 'w') as f:
        f.write(hgrc_data)

    # Copy the requires file, but also add "eden"
    # If the old repository required sqldirstate drop that requirement.
    requires_data = read_config(repo_hg_dir, 'requires', default='')
    requires = set(requires_data.splitlines())
    requires.add('eden')
    requires.discard('sqldirstate')
    with open(os.path.join(eden_hg_dir, 'requires'), 'w') as outf:
        outf.write('\n'.join(sorted(requires)) + '\n')

    # Create the shared and sharedpath files
    with open(os.path.join(eden_hg_dir, 'shared'), 'w') as f:
        f.write('bookmarks\n')
    with open(os.path.join(eden_hg_dir, 'sharedpath'), 'w') as f:
        # No trailing newline here.  This follows mercurial's behavior.
        f.write(repo_hg_dir)

    # Create an empty bookmarks file
    with open(os.path.join(eden_hg_dir, 'bookmarks'), 'w') as f:
        pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('-e', '--eden-extension',
                    action='store', default=None,
                    help='A custom path to the eden extension '
                    '(useful if the extension is not available in $PYTHONPATH)')
    ap.add_argument('repo_type',
                    help='The type of the repo that was cloned: hg or git')
    ap.add_argument('eden_checkout',
                    help='The path to the mounted eden checkout')
    ap.add_argument('repo',
                    help='The path to the original mercurial repository')
    args = ap.parse_args()

    if (args.repo_type != 'hg'):
        # Only Hg is supported by this script.
        return

    repo_hg_dir = os.path.join(args.repo, '.hg')
    eden_hg_dir = os.path.join(args.eden_checkout, '.hg')

    if not os.path.isdir(repo_hg_dir):
        raise Exception('HG repository not found at %s' % repo_hg_dir)
    if os.path.exists(eden_hg_dir):
        raise Exception('%s already exists' % eden_hg_dir)

    # Populate a temporary directory first,
    # then rename it to the real location on success.
    tmp_dir = tempfile.mkdtemp(dir=args.eden_checkout, prefix='.hg-')

    eden_ext_path = args.eden_extension
    if eden_ext_path is None:
        # TODO(mbolin): Call `eden config --get hg.edenextensionpath`
        pass

    try:
        setup_eden_hg_dir(tmp_dir, repo_hg_dir, eden_ext_path)
        os.rename(tmp_dir, eden_hg_dir)
    except BaseException:
        shutil.rmtree(tmp_dir, ignore_errors=True)
        raise

    print('Created %s' % eden_hg_dir)


if __name__ == '__main__':
    rc = main()
    sys.exit(rc)
