# test_repository.py -- tests for repository.py
# Copyright (C) 2007 James Westby <jw+debian@jameswestby.net>
# 
# This program is free software; you can redistribute it and/or
# modify it under the terms of the GNU General Public License
# as published by the Free Software Foundation; version 2
# of the License or (at your option) any later version of 
# the License.
# 
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
# 
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston,
# MA  02110-1301, USA.

import os
import unittest

from dulwich import errors
from dulwich.repo import Repo

missing_sha = 'b91fa4d900e17e99b433218e988c4eb4a3e9a097'

class RepositoryTests(unittest.TestCase):
  
    def open_repo(self, name):
        return Repo(os.path.join(os.path.dirname(__file__),
                          'data/repos', name, '.git'))
  
    def test_simple_props(self):
        r = self.open_repo('a')
        basedir = os.path.join(os.path.dirname(__file__), 'data/repos/a/.git')
        self.assertEqual(r.controldir(), basedir)
        self.assertEqual(r.object_dir(), os.path.join(basedir, 'objects'))
  
    def test_ref(self):
        r = self.open_repo('a')
        self.assertEqual(r.ref('master'),
                         'a90fa2d900a17e99b433217e988c4eb4a2e9a097')
  
    def test_get_refs(self):
        r = self.open_repo('a')
        self.assertEquals({
            'HEAD': 'a90fa2d900a17e99b433217e988c4eb4a2e9a097', 
            'refs/heads/master': 'a90fa2d900a17e99b433217e988c4eb4a2e9a097'
            }, r.get_refs())
  
    def test_head(self):
        r = self.open_repo('a')
        self.assertEqual(r.head(), 'a90fa2d900a17e99b433217e988c4eb4a2e9a097')
  
    def test_get_object(self):
        r = self.open_repo('a')
        obj = r.get_object(r.head())
        self.assertEqual(obj._type, 'commit')
  
    def test_get_object_non_existant(self):
        r = self.open_repo('a')
        self.assertRaises(KeyError, r.get_object, missing_sha)
  
    def test_commit(self):
        r = self.open_repo('a')
        obj = r.commit(r.head())
        self.assertEqual(obj._type, 'commit')
  
    def test_commit_not_commit(self):
        r = self.open_repo('a')
        self.assertRaises(errors.NotCommitError,
                          r.commit, '4f2e6529203aa6d44b5af6e3292c837ceda003f9')
  
    def test_tree(self):
        r = self.open_repo('a')
        commit = r.commit(r.head())
        tree = r.tree(commit.tree)
        self.assertEqual(tree._type, 'tree')
        self.assertEqual(tree.sha().hexdigest(), commit.tree)
  
    def test_tree_not_tree(self):
        r = self.open_repo('a')
        self.assertRaises(errors.NotTreeError, r.tree, r.head())
  
    def test_get_blob(self):
        r = self.open_repo('a')
        commit = r.commit(r.head())
        tree = r.tree(commit.tree())
        blob_sha = tree.entries()[0][2]
        blob = r.get_blob(blob_sha)
        self.assertEqual(blob._type, 'blob')
        self.assertEqual(blob.sha().hexdigest(), blob_sha)
  
    def test_get_blob(self):
        r = self.open_repo('a')
        self.assertRaises(errors.NotBlobError, r.get_blob, r.head())
    
    def test_linear_history(self):
        r = self.open_repo('a')
        history = r.revision_history(r.head())
        shas = [c.sha().hexdigest() for c in history]
        self.assertEqual(shas, [r.head(),
                                '2a72d929692c41d8554c07f6301757ba18a65d91'])
  
    def test_merge_history(self):
        r = self.open_repo('simple_merge')
        history = r.revision_history(r.head())
        shas = [c.sha().hexdigest() for c in history]
        self.assertEqual(shas, ['5dac377bdded4c9aeb8dff595f0faeebcc8498cc',
                                'ab64bbdcc51b170d21588e5c5d391ee5c0c96dfd',
                                '4cffe90e0a41ad3f5190079d7c8f036bde29cbe6',
                                '60dacdc733de308bb77bb76ce0fb0f9b44c9769e',
                                '0d89f20333fbb1d2f3a94da77f4981373d8f4310'])
  
    def test_revision_history_missing_commit(self):
        r = self.open_repo('simple_merge')
        self.assertRaises(errors.MissingCommitError, r.revision_history,
                          missing_sha)
  
    def test_out_of_order_merge(self):
        """Test that revision history is ordered by date, not parent order."""
        r = self.open_repo('ooo_merge')
        history = r.revision_history(r.head())
        shas = [c.sha().hexdigest() for c in history]
        self.assertEqual(shas, ['7601d7f6231db6a57f7bbb79ee52e4d462fd44d1',
                                'f507291b64138b875c28e03469025b1ea20bc614',
                                'fb5b0425c7ce46959bec94d54b9a157645e114f5',
                                'f9e39b120c68182a4ba35349f832d0e4e61f485c'])
  
    def test_get_tags_empty(self):
        r = self.open_repo('ooo_merge')
        self.assertEquals({}, r.get_tags())
