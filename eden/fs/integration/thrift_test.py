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


class ThriftTest(testcase.EdenTestCase):
    def test_list_mounts(self):
        eden = self.init_git_eden()
        client = eden.get_thrift_client()

        mounts = client.listMounts()
        self.assertEqual(1, len(mounts))

        mount = mounts[0]
        self.assertEqual(eden.mount_path, mount.mountPoint)
        # Currently, edenClientPath is not set.
        self.assertEqual('', mount.edenClientPath)
