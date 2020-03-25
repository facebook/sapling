#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest

from eden.cli.mtab import MountInfo, parse_macos_mount_output, parse_mtab


class MTabTest(unittest.TestCase):
    # The diffs for what is written to stdout can be large.
    maxDiff = None

    def test_parse_mtab(self):
        contents = """\
homedir.eden.com:/home109/chadaustin/public_html /mnt/public/chadaustin nfs rw,context=user_u:object_r:user_home_dir_t,relatime,vers=3,rsize=65536,wsize=65536,namlen=255,soft,nosharecache,proto=tcp6,timeo=100,retrans=2,sec=krb5i,mountaddr=2401:db00:fffe:1007:face:0000:0:4007,mountvers=3,mountport=635,mountproto=udp6,local_lock=none,addr=2401:db00:fffe:1007:0000:b00c:0:4007 0 0
squashfuse_ll /mnt/xarfuse/uid-0/2c071047-ns-4026531840 fuse.squashfuse_ll rw,nosuid,nodev,relatime,user_id=0,group_id=0 0 0
bogus line here
edenfs /tmp/eden_test.4rec6drf/mounts/main fuse rw,nosuid,relatime,user_id=138655,group_id=100,default_permissions,allow_other 0 0
"""
        mount_infos = parse_mtab(contents)
        self.assertEqual(3, len(mount_infos))
        one, two, three = mount_infos
        self.assertEqual("edenfs", three.device)
        self.assertEqual("/tmp/eden_test.4rec6drf/mounts/main", three.mount_point)
        self.assertEqual("fuse", three.vfstype)

    def test_parse_mtab_macos(self):
        contents = b"""\
/dev/disk1s1 on / (apfs, local, journaled)
devfs on /dev (devfs, local, nobrowse)
/dev/disk1s4 on /private/var/vm (apfs, local, noexec, journaled, noatime, nobrowse)
map -hosts on /net (autofs, nosuid, automounted, nobrowse)
map auto_home on /home (autofs, automounted, nobrowse)
map -fstab on /Network/Servers (autofs, automounted, nobrowse)
eden@osxfuse0 on /Users/wez/fbsource (osxfuse_eden, nosuid, synchronous)
"""
        self.assertEqual(
            [
                MountInfo(device=b"/dev/disk1s1", mount_point=b"/", vfstype=b"apfs"),
                MountInfo(device=b"devfs", mount_point=b"/dev", vfstype=b"devfs"),
                MountInfo(
                    device=b"/dev/disk1s4",
                    mount_point=b"/private/var/vm",
                    vfstype=b"apfs",
                ),
                MountInfo(
                    device=b"eden@osxfuse0",
                    mount_point=b"/Users/wez/fbsource",
                    vfstype=b"osxfuse_eden",
                ),
            ],
            parse_macos_mount_output(contents),
        )
