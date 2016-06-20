#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import subprocess


def create_git_repo(repoPath):
    '''Create a simple git repo with deterministic properties.

    The structure is:

    - hello (a regular file with content 'hola\n')
    + adir/
    `----- file (a regular file with content 'foo!\n')
    - slink (a symlink that points to 'hello')
    '''
    subprocess.check_call(['git', 'init'], cwd=repoPath)

    hello_file = os.path.join(repoPath, 'hello')
    with open(hello_file, 'w') as f:
        f.write('hola\n')

    os.mkdir(os.path.join(repoPath, 'adir'))
    other_file = os.path.join(repoPath, 'adir', 'file')
    with open(other_file, 'w') as f:
        f.write('foo!\n')

    symlink_name = os.path.join(repoPath, 'slink')
    os.symlink('hello', symlink_name)

    subprocess.check_call(
        [
            'git', 'add', hello_file, other_file, symlink_name
        ],
        cwd=repoPath
    )

    # Specify all arguments to `git commit` to ensure the resulting hashes
    # are the same every time this test is run.
    dummy_name = 'A. Person'
    dummy_email = 'person@example.com'
    dummy_date = '2000-01-01T00:00:00+0000'
    git_commit_args = [
        'git',
        'commit',
        '--message',
        'Initial commit.',
        '--date',
        dummy_date,
        '--author',
        '%s <%s>' % (dummy_name, dummy_email),
    ]
    git_commit_env = {
        'GIT_COMMITTER_NAME': dummy_name,
        'GIT_COMMITTER_EMAIL': dummy_email,
        'GIT_COMMITTER_DATE': dummy_date,
    }
    subprocess.check_call(git_commit_args, env=git_commit_env, cwd=repoPath)
