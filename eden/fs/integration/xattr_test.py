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

class XattrTest(testcase.EdenTestCase):
    def test_get_sha1_xattr(self):
        eden = self.init_git_eden()
        filename = os.path.join(eden.mount_path, 'hello')
        xattr = fs.getxattr(filename, 'user.sha1')
        contents = open(filename).read()
        expected_sha1 = hashlib.sha1(contents).hexdigest()
        self.assertEqual(expected_sha1, xattr)

    def test_listxattr(self):
        eden = self.init_git_eden()
        filename = os.path.join(eden.mount_path, 'hello')
        xattrs = fs.listxattr(filename)
        contents = open(filename).read()
        expected_sha1 = hashlib.sha1(contents).hexdigest()
        self.assertEqual({'user.sha1': expected_sha1}, xattrs)
