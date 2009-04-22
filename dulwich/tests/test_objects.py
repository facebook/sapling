# test_objects.py -- tests for objects.py
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

from dulwich.objects import (
    Blob,
    Tree,
    Commit,
    Tag,
    )

a_sha = '6f670c0fb53f9463760b7295fbb814e965fb20c8'
b_sha = '2969be3e8ee1c0222396a5611407e4769f14e54b'
c_sha = '954a536f7819d40e6f637f849ee187dd10066349'
tree_sha = '70c190eb48fa8bbb50ddc692a17b44cb781af7f6'
tag_sha = '71033db03a03c6a36721efcf1968dd8f8e0cf023'

class BlobReadTests(unittest.TestCase):
    """Test decompression of blobs"""
  
    def get_sha_file(self, obj, base, sha):
        return obj.from_file(os.path.join(os.path.dirname(__file__),
                                          'data', base, sha))
  
    def get_blob(self, sha):
        """Return the blob named sha from the test data dir"""
        return self.get_sha_file(Blob, 'blobs', sha)
  
    def get_tree(self, sha):
        return self.get_sha_file(Tree, 'trees', sha)
  
    def get_tag(self, sha):
        return self.get_sha_file(Tag, 'tags', sha)
  
    def commit(self, sha):
        return self.get_sha_file(Commit, 'commits', sha)
  
    def test_decompress_simple_blob(self):
        b = self.get_blob(a_sha)
        self.assertEqual(b.data, 'test 1\n')
        self.assertEqual(b.sha().hexdigest(), a_sha)
  
    def test_parse_empty_blob_object(self):
        sha = 'e69de29bb2d1d6434b8b29ae775ad8c2e48c5391'
        b = self.get_blob(sha)
        self.assertEqual(b.data, '')
        self.assertEqual(b.sha().hexdigest(), sha)
  
    def test_create_blob_from_string(self):
        string = 'test 2\n'
        b = Blob.from_string(string)
        self.assertEqual(b.data, string)
        self.assertEqual(b.sha().hexdigest(), b_sha)
  
    def test_parse_legacy_blob(self):
        string = 'test 3\n'
        b = self.get_blob(c_sha)
        self.assertEqual(b.data, string)
        self.assertEqual(b.sha().hexdigest(), c_sha)
  
    def test_eq(self):
        blob1 = self.get_blob(a_sha)
        blob2 = self.get_blob(a_sha)
        self.assertEqual(blob1, blob2)
  
    def test_read_tree_from_file(self):
        t = self.get_tree(tree_sha)
        self.assertEqual(t.entries()[0], (33188, 'a', a_sha))
        self.assertEqual(t.entries()[1], (33188, 'b', b_sha))
  
    def test_read_tag_from_file(self):
        t = self.get_tag(tag_sha)
        self.assertEqual(t.object, (Commit, '51b668fd5bf7061b7d6fa525f88803e6cfadaa51'))
        self.assertEqual(t.name,'signed')
        self.assertEqual(t.tagger,'Ali Sabil <ali.sabil@gmail.com>')
        self.assertEqual(t.tag_time, 1231203091)
        self.assertEqual(t.message, 'This is a signed tag\n-----BEGIN PGP SIGNATURE-----\nVersion: GnuPG v1.4.9 (GNU/Linux)\n\niEYEABECAAYFAkliqx8ACgkQqSMmLy9u/kcx5ACfakZ9NnPl02tOyYP6pkBoEkU1\n5EcAn0UFgokaSvS371Ym/4W9iJj6vh3h\n=ql7y\n-----END PGP SIGNATURE-----\n')
  
  
    def test_read_commit_from_file(self):
        sha = '60dacdc733de308bb77bb76ce0fb0f9b44c9769e'
        c = self.commit(sha)
        self.assertEqual(c.tree, tree_sha)
        self.assertEqual(c.parents, ['0d89f20333fbb1d2f3a94da77f4981373d8f4310'])
        self.assertEqual(c.author,
            'James Westby <jw+debian@jameswestby.net>')
        self.assertEqual(c.committer,
            'James Westby <jw+debian@jameswestby.net>')
        self.assertEqual(c.commit_time, 1174759230)
        self.assertEqual(c.commit_timezone, 0)
        self.assertEqual(c.author_timezone, 0)
        self.assertEqual(c.message, 'Test commit\n')
  
    def test_read_commit_no_parents(self):
        sha = '0d89f20333fbb1d2f3a94da77f4981373d8f4310'
        c = self.commit(sha)
        self.assertEqual(c.tree, '90182552c4a85a45ec2a835cadc3451bebdfe870')
        self.assertEqual(c.parents, [])
        self.assertEqual(c.author,
            'James Westby <jw+debian@jameswestby.net>')
        self.assertEqual(c.committer,
            'James Westby <jw+debian@jameswestby.net>')
        self.assertEqual(c.commit_time, 1174758034)
        self.assertEqual(c.commit_timezone, 0)
        self.assertEqual(c.author_timezone, 0)
        self.assertEqual(c.message, 'Test commit\n')
  
    def test_read_commit_two_parents(self):
        sha = '5dac377bdded4c9aeb8dff595f0faeebcc8498cc'
        c = self.commit(sha)
        self.assertEqual(c.tree, 'd80c186a03f423a81b39df39dc87fd269736ca86')
        self.assertEqual(c.parents, ['ab64bbdcc51b170d21588e5c5d391ee5c0c96dfd',
                                       '4cffe90e0a41ad3f5190079d7c8f036bde29cbe6'])
        self.assertEqual(c.author,
            'James Westby <jw+debian@jameswestby.net>')
        self.assertEqual(c.committer,
            'James Westby <jw+debian@jameswestby.net>')
        self.assertEqual(c.commit_time, 1174773719)
        self.assertEqual(c.commit_timezone, 0)
        self.assertEqual(c.author_timezone, 0)
        self.assertEqual(c.message, 'Merge ../b\n')
  
