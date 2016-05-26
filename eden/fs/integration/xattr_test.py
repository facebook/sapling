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

from eden.fs.integration import testcase
from eden.fs.integration import fs
import hashlib
import os


def sha1(value):
    return hashlib.sha1(value).hexdigest()


class XattrTest(testcase.EdenTestCase):
    def test_get_sha1_xattr(self):
        eden = self.init_git_eden()
        filename = os.path.join(eden.mount_path, 'hello')
        xattr = fs.getxattr(filename, 'user.sha1')
        contents = open(filename).read()
        expected_sha1 = sha1(contents)
        self.assertEqual(expected_sha1, xattr)

        # and test what happens as we replace the file contents.
        with open(filename, 'w') as f:
            f.write('foo')
            f.flush()
            self.assertEqual(sha1('foo'),
                             fs.getxattr(filename, 'user.sha1'))

            f.write('bar')
            f.flush()
            self.assertEqual(sha1('foobar'),
                             fs.getxattr(filename, 'user.sha1'))

            f.write('baz')

        self.assertEqual(sha1('foobarbaz'),
                         fs.getxattr(filename, 'user.sha1'))

    def test_listxattr(self):
        eden = self.init_git_eden()
        filename = os.path.join(eden.mount_path, 'hello')
        xattrs = fs.listxattr(filename)
        contents = open(filename).read()
        expected_sha1 = sha1(contents)
        self.assertEqual({'user.sha1': expected_sha1}, xattrs)
