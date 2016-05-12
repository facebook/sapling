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

from libfb.parutil import get_file_path

import binascii
import hashlib
import os.path
import shutil
import subprocess
import tempfile
import unittest

# This comes from gen_srcs in the TARGETS file.
MAIN_BINARY = 'main'
MAIN_PATH = os.path.join('eden/fs/importer/git/test', MAIN_BINARY)
PATH_TO_GIT_IMPORTER = get_file_path(MAIN_PATH)

# Git objects created by TestCase._create_git_repo().
EXPECTED_GIT_TREE = '5e2b1c8cb65669b9be09a205b978470b1a0becfb'
EXPECTED_GIT_BLOB = '5c1b14949828006ed75a3e8858957f86a2f7e2eb'

class TestCase(unittest.TestCase):
    def setUp(self):
        self._rocksdb = tempfile.mkdtemp()
        self._git = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self._rocksdb, ignore_errors=True)
        shutil.rmtree(self._git, ignore_errors=True)

    def test_populate_rocksdb(self):
        '''Runs the Git importer and verifies the contents of the RocksDB.'''
        self._create_git_repo()
        stdout = subprocess.check_output(
                [PATH_TO_GIT_IMPORTER,
                 '--repo', self._git,
                 '--db', self._rocksdb,
                 ])
        self.assertIn('Root object is %s.' % EXPECTED_GIT_TREE, stdout,
                      msg='Should have written expected tree object.')

        # Verify the Blob object in RocksDB.
        blob = self._ldb_get(EXPECTED_GIT_BLOB)
        self.assertEqual('blob 5\x00hola\n', blob)

        # Verify the SHA-1 for the Blob in RocksDB.
        blob_sha1_key = EXPECTED_GIT_BLOB + binascii.hexlify('s')
        expected_sha1 = hashlib.sha1('hola\n').hexdigest()
        sha1 = self._ldb_get(blob_sha1_key)
        self.assertEqual(expected_sha1, binascii.hexlify(sha1))

        # Verify the Tree object in RocksDB.
        tree = self._ldb_get(EXPECTED_GIT_TREE)
        self.assertEqual(tree[:21], 'tree 33\x00100644 hello\x00')
        self.assertEqual(tree[21:], binascii.unhexlify(EXPECTED_GIT_BLOB))

    def _create_git_repo(self):
        subprocess.check_call(['git', 'init'], cwd=self._git)
        hello_file = os.path.join(self._git, 'hello')
        with open(hello_file, 'w') as f:
            f.write('hola\n')
        subprocess.check_call(['git', 'add', hello_file], cwd=self._git)

        # Specify all arguments to `git commit` to ensure the resulting hashes
        # are the same every time this test is run.
        dummy_name = 'A. Person'
        dummy_email = 'person@example.com'
        dummy_date = '2000-01-01T00:00:00+0000'
        git_commit_args = [
            'git', 'commit',
            '--message', 'Initial commit.',
            '--date', dummy_date,
            '--author', '%s <%s>' % (dummy_name, dummy_email),
        ]
        git_commit_env = {
            'GIT_COMMITTER_NAME': dummy_name,
            'GIT_COMMITTER_EMAIL': dummy_email,
            'GIT_COMMITTER_DATE': dummy_date,
        }
        subprocess.check_call(git_commit_args, env=git_commit_env,
                              cwd=self._git)

    def _ldb_get(self, hex_key):
        '''Returns the binary value for the key in the RocksDB.'''
        ldb_args = [
            'ldb',
            '--db=%s' % self._rocksdb,
            'get',
            '--hex',
            '0x%s' % hex_key,
        ]
        stdout = subprocess.check_output(ldb_args)

        # Strip off the leading 0x that ldb prepends to the output.
        self.assertTrue(stdout.startswith('0x'))
        # Also strip off any trailing whitespace before decoding.
        return binascii.unhexlify(stdout[2:].rstrip())
