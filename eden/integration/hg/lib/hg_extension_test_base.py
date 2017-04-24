#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from ...lib import find_executables, hgrepo, testcase
import configparser
import os


def _find_post_clone():
    post_clone = os.environ.get('EDENFS_POST_CLONE_PATH')
    if not post_clone:
        post_clone = os.path.join(find_executables.BUCK_OUT,
                              'gen/eden/hooks/hg/post-clone.par')
    if not os.access(post_clone, os.X_OK):
        msg = ('unable to find post-clone script for integration testing: {!r}'
                .format(post_clone))
        raise Exception(msg)
    return post_clone


def _eden_ext_dir():
    check_locations = [
        # In dev mode, the python_binary link-tree can be found here:
        'buck-out/gen/eden/hg/eden/eden#link-tree',
        # In other modes, we unpack the python archive here:
        'buck-out/gen/eden/hg/eden/eden/output',
    ]
    for location in check_locations:
        hg_ext_dir = os.path.join(find_executables.REPO_ROOT, location,
                                  'hgext3rd/eden')
        if os.path.isdir(hg_ext_dir):
            return hg_ext_dir

    msg = ('unable to find Hg extension for integration testing: {!r}'
            .format(hg_ext_dir))
    raise Exception(msg)


POST_CLONE = _find_post_clone()
EDEN_EXT_DIR = _eden_ext_dir()


class HgExtensionTestBase(testcase.EdenTestCase):
    '''
    A test case class for integration tests that exercise mercurial commands
    inside an eden client.

    This test case sets up two repositories:
    - self.backing_repo:
      This is the underlying mercurial repository that provides the data for
      the eden mount point.  This has to be populated with an initial commit
      before the eden client is configured, but after initalization most of the
      test interaction will generally be with self.repo instead.

    - self.repo
      This is the hg repository in the eden client.  This is the repository
      where most mercurial commands are actually being tested.
    '''
    def setup_eden_test(self):
        super().setup_eden_test()

        # Create the backing repository
        self.backing_repo_name = 'backing_repo'
        self.mount = os.path.join(self.mounts_dir, self.backing_repo_name)
        self.backing_repo = self.create_repo(self.backing_repo_name,
                                             hgrepo.HgRepository)
        self.populate_backing_repo(self.backing_repo)

        self.eden.add_repository(self.backing_repo_name, self.backing_repo.path)
        # Edit the edenrc file to set up post-clone hooks that will correctly
        # populate the .hg directory inside the eden client.
        self.amend_edenrc_before_clone()
        self.eden.clone(self.backing_repo_name, self.mount)

        # Now create the repository object that refers to the eden client
        self.repo = hgrepo.HgRepository(self.mount)

    def populate_backing_repo(self, repo):
        raise NotImplementedError('individual test classes must implement '
                                  'populate_backing_repo()')

    def amend_edenrc_before_clone(self):
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

        config['repository %s' % self.backing_repo_name]['hooks'] = hooks_dir

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
        return self.repo.hg(*args, stdout_charset=stdout_charset)

    def status(self):
        '''Returns the output of `hg status` as a string.'''
        return self.repo.status()

    def assert_status(self, expected, msg=None, check_ignored=True):
        '''Returns the output of `hg status` as a string.'''
        args = ['status', '--print0']
        if check_ignored:
            args.append('-mardui')

        output = self.hg(*args)
        actual_status = {}
        for entry in output.split('\0'):
            if not entry:
                continue
            flag = entry[0]
            path = entry[2:]
            actual_status[path] = flag

        self.assertDictEqual(expected, actual_status)

    def assert_status_empty(self, msg=None, check_ignored=True):
        '''Ensures that `hg status` reports no modifications.'''
        self.assert_status({}, msg=msg, check_ignored=check_ignored)

    def touch(self, path):
        '''Touch the file at the specified path relative to the clone.'''
        fullpath = self.get_path(path)
        with open(fullpath, 'a'):
            os.utime(fullpath)

    def write_file(self, path, contents, mode=0o644):
        '''Create or overwrite a file with the given contents.'''
        fullpath = self.get_path(path)
        with open(fullpath, 'w') as f:
            f.write(contents)
        os.chmod(fullpath, mode)

    def read_file(self, path):
        '''Read the file with the specified path inside the eden repository,
        and return its contents.
        '''
        fullpath = self.get_path(path)
        with open(fullpath, 'r') as f:
            return f.read()

    def mkdir(self, path):
        '''Call mkdir for the specified path relative to the clone.'''
        fullpath = self.get_path(path)
        os.mkdir(fullpath)

    def rm(self, path):
        '''Unlink the file at the specified path relative to the clone.'''
        os.unlink(self.get_path(path))
