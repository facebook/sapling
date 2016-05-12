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
import shutil
import tempfile
import unittest
from eden.fs.integration import edenclient
from eden.fs.integration import gitrepo

if hasattr(shutil, 'which'):
    which = shutil.which
else:
    from distutils import spawn
    which = spawn.find_executable

@unittest.skipIf(not which("fusermount"), "fuse is not installed")
class EdenTestCase(unittest.TestCase):
    def setUp(self):
        self._paths_to_clean = []
        self._eden_instances = []

    def tearDown(self):
        for inst in self._eden_instances:
            inst.cleanup()
        for path in self._paths_to_clean:
            shutil.rmtree(path, ignore_errors=True)

    def init_eden(self, repo_path, **kwargs):
        '''Create and initialize an eden client for a given repo.

        @return EdenClient
        '''

        inst = edenclient.EdenClient()
        inst.init(repo_path, *kwargs)
        self._eden_instances.append(inst)
        return inst

    def init_git_repo(self):
        '''initializes a standard git repo in a temporary dir.

        The temporary dir will be automatically cleaned up.

        @return string the dir containing the repo.
        '''

        repo_path = tempfile.mkdtemp(prefix='eden_test.repo.')
        gitrepo.create_git_repo(repo_path)
        self._paths_to_clean.append(repo_path)
        return repo_path

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
