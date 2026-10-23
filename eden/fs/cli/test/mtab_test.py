#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import errno
import multiprocessing.context
import unittest
from typing import Optional
from unittest.mock import create_autospec

from eden.fs.cli.mtab import (
    LinuxMountTable,
    MountInfo,
    parse_macos_mount_output,
    parse_mtab,
)


class MTabTest(unittest.TestCase):
    # The diffs for what is written to stdout can be large.
    maxDiff: Optional[int] = None

    def test_parse_mtab(self) -> None:
        contents = b"""\
homedir.eden.com:/home109/chadaustin/public_html /mnt/public/chadaustin nfs rw,context=user_u:object_r:user_home_dir_t,relatime,vers=3,rsize=65536,wsize=65536,namlen=255,soft,nosharecache,proto=tcp6,timeo=100,retrans=2,sec=krb5i,mountaddr=2401:db00:fffe:1007:face:0000:0:4007,mountvers=3,mountport=635,mountproto=udp6,local_lock=none,addr=2401:db00:fffe:1007:0000:b00c:0:4007 0 0
squashfuse_ll /mnt/xarfuse/uid-0/2c071047-ns-4026531840 fuse.squashfuse_ll rw,nosuid,nodev,relatime,user_id=0,group_id=0 0 0
bogus line here
edenfs: /tmp/eden_test.4rec6drf/mounts/main fuse rw,nosuid,relatime,user_id=138655,group_id=100,default_permissions,allow_other 0 0
"""
        mount_infos = parse_mtab(contents)
        self.assertEqual(3, len(mount_infos))
        one, two, three = mount_infos
        self.assertEqual(b"edenfs:", three.device)
        self.assertEqual(b"/tmp/eden_test.4rec6drf/mounts/main", three.mount_point)
        self.assertEqual(b"fuse", three.vfstype)

    def test_parse_mtab_macos(self) -> None:
        contents = b"""\
/dev/disk1s1 on / (apfs, local, journaled)
devfs on /dev (devfs, local, nobrowse)
/dev/disk1s4 on /private/var/vm (apfs, local, noexec, journaled, noatime, nobrowse)
map -hosts on /net (autofs, nosuid, automounted, nobrowse)
map auto_home on /home (autofs, automounted, nobrowse)
map -fstab on /Network/Servers (autofs, automounted, nobrowse)
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
            ],
            parse_macos_mount_output(contents),
        )


class CloseHangingProcessTest(unittest.TestCase):
    def test_helper_that_survives_kill(self) -> None:
        helper = create_autospec(multiprocessing.context.SpawnProcess, instance=True)
        helper.is_alive.return_value = True
        helper.close.side_effect = ValueError(
            "Cannot close a process while it is still running. "
            "You should first call join() or terminate()."
        )
        timeout_error = OSError(
            errno.ETIMEDOUT, "Stating the mount timed out, mount point hangs"
        )

        with self.assertRaises(ValueError):
            LinuxMountTable().close_hanging_process(helper, timeout_error)

        helper.terminate.assert_called_once_with()
        helper.kill.assert_called_once_with()
        self.assertEqual(helper.join.call_count, 2)
