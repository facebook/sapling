#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from ...lib import find_executables, hgrepo, testcase
import configparser
import os


def _find_post_clone():
    post_clone = os.path.join(find_executables.BUCK_OUT,
                              'gen/eden/hooks/hg/post-clone.par')
    if not os.access(post_clone, os.X_OK):
        msg = ('unable to find post-clone script for integration testing: {!r}'
                .format(post_clone))
        raise Exception(msg)
    return post_clone


def _eden_ext_dir():
    hg_ext_dir = os.path.join(find_executables.REPO_ROOT, 'eden/hg/eden')
    if not os.path.isdir(hg_ext_dir):
        msg = ('unable to find Hg extension for integration testing: {!r}'
                .format(hg_ext_dir))
        raise Exception(msg)
    return hg_ext_dir


POST_CLONE = _find_post_clone()
EDEN_EXT_DIR = _eden_ext_dir()


class HgExtensionTestBase(testcase.EdenHgTest):
    def amend_edenrc_before_clone(self):
        # Most likely, subclasses will want to use self.repo_for_mount rather
        # than self.repo.
        self.repo_for_mount = hgrepo.HgRepository(self.mount)

        # This is a poor man's version of the generate-hooks-dir script.
        hooks_dir = os.path.join(self.tmp_dir, 'the_hooks')
        os.mkdir(hooks_dir)
        post_clone_hook = os.path.join(hooks_dir, 'post-clone')
        os.symlink(POST_CLONE, post_clone_hook)

        edenrc = os.path.join(os.environ['HOME'], '.edenrc')
        config = configparser.ConfigParser()
        config.read(edenrc)

        config['hooks'] = {}
        config['hooks']['hg.edenextension'] = EDEN_EXT_DIR

        config['repository %s' % self.repo_name]['hooks'] = hooks_dir

        with open(edenrc, 'w') as f:
            config.write(f)

    def get_path(self, path):
        '''Resolves the path against self.mount.'''
        return os.path.join(self.mount, path)

    def hg(self, *args, stdout_charset='utf-8'):
        '''Runs `hg.real` with the specified args in the Eden mount.

        Returns the stdout decoded as a utf8 string. To use a different charset,
        specify the `stdout_charset` as a keyword argument.
        '''
        return self.repo_for_mount.hg(*args, stdout_charset=stdout_charset)

    def status(self):
        '''Returns the output of `hg status` as a string.'''
        return self.repo_for_mount.status()

    def touch(self, path):
        '''Touch the file at the specified path relative to the clone.'''
        fullpath = self.get_path(path)
        with open(fullpath, 'a'):
            os.utime(fullpath)

    def rm(self, path):
        '''Unlink the file at the specified path relative to the clone.'''
        os.unlink(self.get_path(path))
