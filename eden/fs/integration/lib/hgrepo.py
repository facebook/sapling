#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
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
import subprocess
import tempfile

from . import repobase


class HgRepository(repobase.Repository):
    def __init__(self, path):
        super().__init__(path)
        self.hg_environment = os.environ.copy()
        self.hg_environment['HGPLAIN'] = '1'
        self.hg_bin = distutils.spawn.find_executable(
            'hg.real') or distutils.spawn.find_executable('hg')

    def hg(self, *args, stdout_charset='utf-8'):
        cmd = [self.hg_bin] + list(args)
        completed_process = subprocess.run(cmd, stdout=subprocess.PIPE,
                                           stderr=subprocess.PIPE,
                                           check=True, cwd=self.path,
                                           env=self.hg_environment)
        return completed_process.stdout.decode(stdout_charset)

    def init(self):
        self.hg('init')

    def get_type(self):
        return 'hg'

    def get_head_hash(self):
        return self.hg('log', '-r.', '-T{node}')

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
