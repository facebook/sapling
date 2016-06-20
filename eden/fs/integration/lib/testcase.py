#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import shutil
import tempfile
import unittest
from . import edenclient
from . import gitrepo

if hasattr(shutil, 'which'):
    which = shutil.which
else:
    from distutils import spawn
    which = spawn.find_executable

@unittest.skipIf(not which("fusermount"), "fuse is not installed")
class EdenTestCase(unittest.TestCase):
    def setUp(self):
        self.tmp_dir = tempfile.mkdtemp(prefix='eden_test.')
        self._eden_instances = {}
        self._next_client_id = 1

    def tearDown(self):
        errors = []
        for inst in self._eden_instances.values():
            try:
                inst.cleanup()
            except Exception as ex:
                errors.append(ex)

        shutil.rmtree(self.tmp_dir, ignore_errors=True)
        self.tmp_dir = None

        # Re-raise any errors that occurred, after we finish
        # trying to clean up our directories.
        if errors:
            raise errors[0]

    def new_tmp_dir(self, prefix='edentmp'):
        return tempfile.mkdtemp(prefix=prefix, dir=self.tmp_dir)

    def init_eden(self, repo_path, **kwargs):
        '''Create and initialize an eden client for a given repo.

        @return EdenClient
        '''
        inst = edenclient.EdenClient(self)
        inst.init(repo_path, *kwargs)
        self._eden_instances[id(inst)] = inst
        return inst

    def register_eden_client(self, client):
        '''
        This function is called by the EdenClient constructor to register
        a new EdenClient object.

        This shouldn't be called by anyone else other than the EdenClient
        constructor.
        '''
        self._eden_instances[id(client)] = client
        client_name = 'client{}'.format(self._next_client_id)
        self._next_client_id += 1
        return client_name

    def init_git_repo(self):
        '''Create a simple git repo with deterministic properties.

        The structure is:

          - hello (a regular file with content 'hola\n')
          + adir/
          `----- file (a regular file with content 'foo!\n')
          - slink (a symlink that points to 'hello')

        @return string the dir containing the repo.
        '''
        repo_path = self.new_tmp_dir('git')
        repo = gitrepo.GitRepository(repo_path)
        repo.init()

        repo.write_file('hello', 'hola\n')
        repo.write_file('adir/file', 'foo!\n')
        repo.symlink('slink', 'hello')
        repo.commit('Initial commit.')
        return repo.path

    def init_git_eden(self):
        '''Initializes a git repo and an eden client for it.

        The git repo will be initialized in a temporary directory.
        The directory is visible via the returned eden client.
        The temporary dirs associated with the client and repo
        will be cleaned up automatically.

        @return EdenClient
        '''
        repo_path = self.init_git_repo()
        return self.init_eden(repo_path)
