#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import json
import os
from .lib import gitrepo
from .lib import hgrepo
from .lib import testcase


@testcase.eden_repo_test
class RemountTest:
    '''Exercise some fundamental properties of the filesystem.

    Listing directories, checking stat information, asserting
    that the filesystem is reporting the basic information
    about the sample git repo and that it is correct are all
    things that are appropriate to include in this test case.
    '''
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.write_file('adir/file', 'foo!\n')
        self.repo.symlink('slink', 'hello')
        self.repo.commit('Initial commit.')

    def test_remount_basic(self):
        # Mount multiple clients
        for i in range(5):
            self.eden.clone(self.repo_name, self.mount + '-' + str(i))

        self.eden.shutdown()
        self.eden.start()

        # Verify that clients are remounted on startup
        for i in range(5):
            entries = sorted(os.listdir(self.mount + '-' + str(i)))
            self.assertEqual(['adir', 'hello', 'slink'], entries)

        # Verify that default repo created by EdenRepoTestBase is remounted
        entries = sorted(os.listdir(self.mount))
        self.assertEqual(['adir', 'hello', 'slink'], entries)

    def test_git_and_hg(self):
        # Create git and hg repositories for mounting
        repo_names = {'git': 'git_repo', 'hg': 'hg_repo'}
        git_repo = self.create_repo(repo_names['git'], gitrepo.GitRepository)
        hg_repo = self.create_repo(repo_names['hg'], hgrepo.HgRepository)

        git_repo.write_file('hello', 'hola\n')
        git_repo.commit('Initial commit.')

        hg_repo.write_file('hello', 'hola\n')
        hg_repo.commit('Initial commit.')

        self.eden.add_repository(repo_names['git'], git_repo.path)
        self.eden.add_repository(repo_names['hg'], hg_repo.path)

        # Mount git and hg clients
        for name in repo_names.values():
            for i in range(3):
                self.eden.clone(name, os.path.join(self.mounts_dir,
                                                   name + '-' + str(i)))

        self.eden.shutdown()
        self.eden.start()

        # Verify that clients are remounted on startup
        for name in repo_names.values():
            for i in range(3):
                mount_path = os.path.join(self.mounts_dir, name + '-' + str(i))
                entries = sorted(os.listdir(mount_path))
                self.assertEqual(['hello'], entries)

                hello = os.path.join(mount_path, 'hello')
                with open(hello, 'r') as f:
                    self.assertEqual('hola\n', f.read())

    def test_partial_unmount(self):
        # Mount multiple clients
        for i in range(5):
            self.eden.clone(self.repo_name, self.mount + '-' + str(i))

        # Unmount a client
        self.eden.unmount(self.mount)
        self.assertFalse(os.path.exists(self.mount))

        self.eden.shutdown()
        self.eden.start()

        # Verify that clients that were still mounted at shutdown are remounted
        for i in range(5):
            entries = sorted(os.listdir(self.mount + '-' + str(i)))
            self.assertEqual(['adir', 'hello', 'slink'], entries)

        # Verify that unmounted client is not remounted
        self.assertFalse(os.path.exists(self.mount))

    def test_restart_twice(self):
        # Mount multiple clients
        for i in range(5):
            self.eden.clone(self.repo_name, self.mount + '-' + str(i))

        self.eden.shutdown()
        self.eden.start()

        # Unmount clients
        self.eden.unmount(self.mount)
        self.eden.unmount(self.mount + '-3')

        self.eden.shutdown()
        self.eden.start()

        # Verify that clients that were still mounted at shutdown are remounted
        for i in range(5):
            if i == 3:
                continue
            entries = sorted(os.listdir(self.mount + '-' + str(i)))
            self.assertEqual(['adir', 'hello', 'slink'], entries)

        # Verify that unmounted clients are not remounted
        self.assertFalse(os.path.exists(self.mount))
        self.assertFalse(os.path.exists(self.mount + "3"))

    def test_empty_config_json(self):
        for i in range(5):
            self.eden.clone(self.repo_name, self.mount + '-' + str(i))

        # Clear the contents of config.json file
        open(os.path.join(self.eden_dir, 'config.json'), 'w').close()

        self.eden.shutdown()
        self.eden.start()

        # Verify that no clients are remounted. No errors should be thrown here
        for i in range(5):
            entries = sorted(os.listdir(self.mount + '-' + str(i)))
            self.assertEqual([], entries)

        entries = sorted(os.listdir(self.mount))
        self.assertEqual([], entries)

    def test_deleted_config_json(self):
        for i in range(5):
            self.eden.clone(self.repo_name, self.mount + '-' + str(i))

        # Delete the config.json file
        os.remove(os.path.join(self.eden_dir, 'config.json'))

        self.eden.shutdown()
        self.eden.start()

        # Verify that no clients are remounted. No errors should be thrown here
        for i in range(5):
            entries = sorted(os.listdir(self.mount + '-' + str(i)))
            self.assertEqual([], entries)

        entries = sorted(os.listdir(self.mount))
        self.assertEqual([], entries)

    def test_incorrect_config_json(self):
        for i in range(5):
            self.eden.clone(self.repo_name, self.mount + '-' + str(i))

        # Reload config.json file with incorrect data
        config_data = {self.mounts_dir + '/incorrectdir1': 'jskdnbailreylbflhv',
                       self.mounts_dir + '/incorrectdir2': 'ndjnhaleruybfialus'}

        with open(os.path.join(self.eden_dir, 'config.json'), 'w') as f:
            json.dump(config_data, f, indent=2, sort_keys=True)
            f.write('\n')

        self.eden.shutdown()
        self.eden.start()

        # Verify that no clients are remounted. No errors should be thrown here
        for i in range(5):
            entries = sorted(os.listdir(self.mount + '-' + str(i)))
            self.assertEqual([], entries)

        entries = sorted(os.listdir(self.mount))
        self.assertEqual([], entries)

        # Incorrect mount paths from config.json should not have been created
        for incorrect_mount in config_data:
            self.assertFalse(os.path.isdir(incorrect_mount))

    def test_bad_config_json(self):
        for i in range(5):
            self.eden.clone(self.repo_name, self.mount + '-' + str(i))

        # Reload config.json file with random data
        with open(os.path.join(self.eden_dir, 'config.json'), 'w') as f:
            f.write('njfaeriurbvailuwrawikc\n')

        self.eden.shutdown()
        self.eden.start()

        # Verify that no clients are remounted. No errors should be thrown here
        for i in range(5):
            entries = sorted(os.listdir(self.mount + '-' + str(i)))
            self.assertEqual([], entries)

        entries = sorted(os.listdir(self.mount))
        self.assertEqual([], entries)
