#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals

import distutils.spawn
import os
import shlex
import subprocess
import tempfile

from . import repobase


class HgError(subprocess.CalledProcessError):
    '''
    A wrapper around subprocess.CalledProcessError that also includes
    includes the process's stderr when converted to a string.
    '''
    def __init__(self, orig):
        super().__init__(orig.returncode, orig.cmd,
                         output=orig.output, stderr=orig.stderr)

    def __str__(self):
        if not self.stderr:
            return super().__str__()

        cmd_str = ' '.join(shlex.quote(arg) for arg in self.cmd)

        stderr_str = self.stderr
        if isinstance(self.stderr, bytes):
            stderr_str = self.stderr.decode('utf-8', errors='replace')

        # Indent the stderr output just to help indicate where it starts
        # and ends in the test output.
        stderr_str = stderr_str.replace('\n', '\n  ')

        msg = 'Command [%s] failed with status %s\nstderr:\n  %s' % (
            cmd_str, self.returncode, stderr_str)
        return msg


class HgRepository(repobase.Repository):
    def __init__(self, path):
        super().__init__(path)
        self.hg_environment = os.environ.copy()
        self.hg_environment['HGPLAIN'] = '1'
        self.hg_bin = distutils.spawn.find_executable(
            'hg.real') or distutils.spawn.find_executable('hg')

    def hg(self, *args, stdout_charset='utf-8', stdout=subprocess.PIPE,
           stderr=subprocess.PIPE):
        cmd = [self.hg_bin] + list(args)
        try:
            completed_process = subprocess.run(cmd, stdout=stdout,
                                               stderr=stderr,
                                               check=True, cwd=self.path,
                                               env=self.hg_environment)
        except subprocess.CalledProcessError as ex:
            raise HgError(ex) from ex
        if completed_process.stdout is not None:
            return completed_process.stdout.decode(stdout_charset)

    def init(self):
        self.hg('init')

    def get_type(self):
        return 'hg'

    def get_head_hash(self):
        return self.hg('log', '-r.', '-T{node}')

    def get_canonical_root(self):
        return self.path

    def add_file(self, path):
        # add_file() may be called for files that are already tracked.
        # hg will print a warning, but this is fine.
        self.hg('add', path)

    def commit(self,
               message,
               author_name=None,
               author_email=None,
               date=None):
        if author_name is None:
            author_name = self.author_name
        if author_email is None:
            author_email = self.author_email
        if date is None:
            date = self.get_commit_time()
            # Mercurial's internal format of <unix_timestamp> <timezone>
            date_str = '{} 0'.format(int(date.timestamp()))

        user_config = 'ui.username={} <{}>'.format(author_name, author_email)

        with tempfile.NamedTemporaryFile(prefix='eden_commit_msg.',
                                         mode='w',
                                         encoding='utf-8') as msgf:
            msgf.write(message)
            msgf.flush()
            self.hg('commit',
                    '--config', user_config,
                    '--date', date_str,
                    '--logfile', msgf.name)

        # Get the commit ID and return it
        return self.hg('log', '-T{node}', '-r.')

    def status(self):
        '''Returns the output of `hg status` as a string.'''
        return self.hg('status')

    def update(self, rev, clean=False):
        if clean:
            args = ['update', '--clean', rev]
        else:
            args = ['update', rev]
        self.hg(*args, stdout=None, stderr=None)
