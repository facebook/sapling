#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import datetime
import distutils.spawn
import os
import shlex
import subprocess
import tempfile
from typing import Any, List, Optional

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
        '''
        If hgrc is specified, it will be used as the value of the HGRCPATH
        environment variable when `hg` is run.
        '''
        super().__init__(path)
        self.hg_environment = os.environ.copy()
        # Drop any environment variables starting with 'HG'
        # to ensure the user's environment does not affect the tests
        self.hg_environment = dict((k, v) for k, v in os.environ.items()
                                   if not k.startswith('HG'))
        self.hg_environment['HGPLAIN'] = '1'
        # Set HGRCPATH to make sure we aren't affected by the local system's
        # mercurial settings from /etc/mercurial/
        self.hg_environment['HGRCPATH'] = ''
        self.hg_bin = distutils.spawn.find_executable(
            'hg.real') or distutils.spawn.find_executable('hg')

    def hg(
        self,
        *args: str,
        stdout_charset: str = 'utf-8',
        stdout: Any = subprocess.PIPE,
        stderr: Any = subprocess.PIPE,
        shell: bool = False,
        hgeditor: Optional[str] = None,
        cwd: Optional[str] = None
    ) -> Optional[str]:
        if shell:
            cmd = self.hg_bin + ' ' + args[0]
        else:
            cmd = [self.hg_bin] + list(args)
        env = self.hg_environment
        if hgeditor is not None:
            env = dict(env)
            env['HGEDITOR'] = hgeditor

        if cwd is None:
            cwd = self.path
        try:
            completed_process = subprocess.run(cmd, stdout=stdout,
                                               stderr=stderr,
                                               check=True, cwd=cwd,
                                               env=env, shell=shell)
        except subprocess.CalledProcessError as ex:
            raise HgError(ex) from ex
        if completed_process.stdout is not None:
            return completed_process.stdout.decode(stdout_charset)
        else:
            return None

    def init(self, hgrc=None):
        '''
        Initialize a new hg repository by running 'hg init'

        The hgrc parameter may be a configparser.ConfigParser() object
        describing configuration settings that should be added to the
        repository's .hg/hgrc file.
        '''
        self.hg('init')
        if hgrc is not None:
            hgrc_path = os.path.join(self.path, '.hg', 'hgrc')
            with open(hgrc_path, 'a') as f:
                hgrc.write(f)

    def get_type(self):
        return 'hg'

    def get_head_hash(self):
        return self.hg('log', '-r.', '-T{node}')

    def get_canonical_root(self):
        return self.path

    def add_files(self, paths: List[str]) -> None:
        # add_files() may be called for files that are already tracked.
        # hg will print a warning, but this is fine.
        self.hg('add', *paths)

    def commit(self,
               message: str,
               author_name: Optional[str]=None,
               author_email: Optional[str]=None,
               date: Optional[datetime.datetime]=None,
               amend: bool=False) -> str:
        '''
        - message Commit message to use.
        - author_name Author name to use: defaults to self.author_name.
        - author_email Author email to use: defaults to self.author_email.
        - date datetime.datetime to use for the commit. Defaults to
          self.get_commit_time().
        - amend If true, adds the `--amend` argument.
        '''
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

            args = [
                'commit',
                '--config', user_config,
                '--date', date_str,
                '--logfile', msgf.name,
            ]
            if amend:
                args.append('--amend')

            # Do not capture stdout or stderr when running "hg commit"
            # This allows its output to show up in the test logs.
            self.hg(*args, stdout=None, stderr=None)

        # Get the commit ID and return it
        return self.hg('log', '-T{node}', '-r.')

    def log(self, template='{node}', revset='::.'):
        '''Runs `hg log` with the specified template and revset.

        Returns the log output, as a list with one entry per commit.'''
        # Append a separator to the template so we can split up the entries
        # afterwards.  Use a slightly more complex string rather than just a
        # single nul byte, just in case the caller uses internal nuls in their
        # template to split fields.
        escaped_delimiter = r'\0-+-\0'
        delimiter = '\0-+-\0'
        assert escaped_delimiter not in template
        template += escaped_delimiter
        output = self.hg('log', '-T', template, '-r', revset)
        return output.split(delimiter)[:-1]

    def status(self):
        '''Returns the output of `hg status` as a string.'''
        return self.hg('status')

    def update(self, rev, clean=False):
        if clean:
            args = ['update', '--clean', rev]
        else:
            args = ['update', rev]
        self.hg(*args, stdout=None, stderr=None)

    def reset(self, rev, keep=True):
        if keep:
            args = ['reset', '--keep', rev]
        else:
            args = ['reset', rev]
        self.hg(*args, stdout=None, stderr=None)
