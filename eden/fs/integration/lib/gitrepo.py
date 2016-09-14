#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import distutils.spawn
import os
import subprocess
import tempfile
import time

from . import repobase


class GitRepository(repobase.Repository):
    def __init__(self, path):
        super().__init__(path)
        self.git_bin = distutils.spawn.find_executable(
            'git.real') or distutils.spawn.find_executable('git')

    def git(self, *args, stdout_charset='utf-8', **kwargs):
        '''
        Invoke a git command inside the repository.

        All non-keyword arguments are treated as arguments to git.

        A keyword argument of "env" can be used to specify a dictionary of
        additional environment variables to be passed to git.  (These will be
        added to the current environment.)

        "env" is currently the only valid keyword argument.

        Example usage:

          repo.git('commit', '-m', 'my new commit',
                   env={'GIT_AUTHOR_NAME': 'John Doe'})
        '''
        cmd = [self.git_bin] + list(args)

        env = os.environ.copy()
        env_args = kwargs.pop('env', None)
        if env_args is not None:
            env.update(env_args)

        if kwargs:
            raise Exception('unexpected keyword argumnts to git(): %r' %
                            list(kwargs.keys))

        completed_process = subprocess.run(cmd, stdout=subprocess.PIPE,
                                           stderr=subprocess.PIPE,
                                           check=True, cwd=self.path,
                                           env=env)
        return completed_process.stdout.decode(stdout_charset)

    def init(self):
        self.git('init')

    def get_type(self):
        return 'git'

    def get_head_hash(self):
        return self.git('rev-parse', 'HEAD').rstrip()

    def add_file(self, path):
        self.git('add', path)

    def commit(self,
               message,
               author_name=None,
               author_email=None,
               date=None,
               committer_name=None,
               committer_email=None,
               committer_date=None):
        if author_name is None:
            author_name = self.author_name
        if author_email is None:
            author_email = self.author_email
        if date is None:
            date = self.get_commit_time()
            date_str = time.strftime('%Y-%m-%dT%H:%M:%S%z',
                                     date.utctimetuple())
        if committer_name is None:
            committer_name = author_name
        if committer_email is None:
            committer_email = author_email
        if committer_date is None:
            committer_date = date
            committer_date_str = time.strftime('%Y-%m-%dT%H:%M:%S%z',
                                               committer_date.utctimetuple())

        # Specify all arguments to `git commit` to ensure the resulting hashes
        # are the same every time this test is run.
        git_commit_env = {
            'GIT_AUTHOR_NAME': author_name,
            'GIT_AUTHOR_EMAIL': author_email,
            'GIT_AUTHOR_DATE': date_str,
            'GIT_COMMITTER_NAME': committer_name,
            'GIT_COMMITTER_EMAIL': committer_email,
            'GIT_COMMITTER_DATE': committer_date_str,
        }

        with tempfile.NamedTemporaryFile(prefix='eden_commit_msg.',
                                         mode='w',
                                         encoding='utf-8') as msgf:
            msgf.write(message)
            msgf.flush()
            self.git('commit', '-F', msgf.name, env=git_commit_env)
