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

import datetime
import errno
import os


class Repository(object):
    def __init__(self, path):
        self.path = path

        # Default author and timestamp info for commits
        self.author_name = 'A. Person'
        self.author_email = 'person@example.com'
        self.commit_time = datetime.datetime(year=2000, month=1, day=1)
        self.commit_time_delta = datetime.timedelta(seconds=1)

    def get_commit_time(self):
        '''
        Get a datetime object to use for the next commit.

        Rather than using real wall clock time, we use an internally maintained
        date to ensure that we get the same commit hashes across repeated test
        runs.

        The date is advanced for each commit made.
        '''
        current = self.commit_time
        self.commit_time += self.commit_time_delta
        return current

    def init(self):
        raise NotImplementedError('subclasses must implement init()')

    def get_type(self):
        '''Returns the type of this repo as a string: "git" or "hg".'''
        raise NotImplementedError('subclasses must implement get_type()')

    def get_head_hash(self):
        '''Returns the 40-character hex hash for HEAD.'''
        raise NotImplementedError('subclasses must implement get_head_hash()')

    def add_file(self, path):
        raise NotImplementedError('subclasses must implement add_file()')

    def get_path(self, *args):
        for arg in args:
            assert not os.path.isabs(arg), 'must not be absolute: %r' % (arg, )
        return os.path.join(self.path, *args)

    def get_canonical_root(self):
        '''Returns cwd to use when calling scm commands.'''
        raise NotImplementedError(
            'subclasses must implement get_canonical_root()'
        )

    def mkdir(self, path):
        full_path = self.get_path(path)
        try:
            os.makedirs(full_path)
        except OSError as ex:
            if ex.errno != errno.EEXIST:
                raise

    def make_parent_dir(self, path):
        dirname = os.path.dirname(path)
        if dirname:
            self.mkdir(dirname)

    def write_file(self, path, contents, mode=None, add=True):
        '''
        Create or overwrite a file with the given contents.
        '''
        self.make_parent_dir(path)

        if mode is None:
            mode = 0o644

        full_path = self.get_path(path)
        with open(full_path, 'w') as f:
            f.write(contents)

        os.chmod(full_path, mode)

        if add:
            self.add_file(path)

    def symlink(self, path, contents, add=True):
        '''
        Create a symlink at the specified path, pointed at the given
        destination path contents.
        '''
        self.make_parent_dir(path)
        full_path = self.get_path(path)
        try:
            os.unlink(full_path)
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise

        os.symlink(contents, full_path)
        if add:
            self.add_file(path)
