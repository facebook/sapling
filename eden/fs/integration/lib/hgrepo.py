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

import os
import subprocess
import tempfile

from . import repobase


class HgRepository(repobase.Repository):
    def __init__(self, path):
        super().__init__(path)
        self.hg_environment = os.environ.copy()
        self.hg_environment['HGPLAIN'] = '1'

    def hg(self, *args):
        cmd = ['hg'] + list(args)
        subprocess.check_call(cmd, cwd=self.path, env=self.hg_environment)

    def init(self):
        self.hg('init')

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
