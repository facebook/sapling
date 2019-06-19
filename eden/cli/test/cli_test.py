#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest
from pathlib import Path

from eden.cli import main as main_mod
from eden.cli.config import EdenCheckout, EdenInstance
from facebook.eden.ttypes import MountInfo, MountState

from .lib.output import TestOutput


class ListTest(unittest.TestCase):
    def test_no_mounts(self):
        out = TestOutput()
        mounts = main_mod.ListCmd.combine_mount_info({}, [])
        main_mod.ListCmd.print_mounts(out, mounts)
        self.assertEqual(out.getvalue(), "")

    def test_list_mounts(self):
        self.maxDiff = None

        thrift_mounts = [
            MountInfo(
                mountPoint=b"/data/users/johndoe/mercurial",
                edenClientPath=b"/home/johndoe/.eden/clients/mercurial",
                state=MountState.RUNNING,
            ),
            MountInfo(
                mountPoint=b"/data/users/johndoe/git",
                edenClientPath=b"/home/johndoe/.eden/clients/git",
                state=MountState.SHUTTING_DOWN,
            ),
            MountInfo(
                mountPoint=b"/data/users/johndoe/apache",
                edenClientPath=b"/home/johndoe/.eden/clients/apache",
                state=MountState.RUNNING,
            ),
            MountInfo(
                mountPoint=b"/data/users/johndoe/configs",
                edenClientPath=b"/home/johndoe/.eden/clients/configs",
                state=MountState.INITIALIZING,
            ),
            MountInfo(
                mountPoint=b"/data/users/johndoe/repos/linux",
                edenClientPath=b"/home/johndoe/.eden/clients/linux",
                state=MountState.RUNNING,
            ),
            MountInfo(
                mountPoint=b"/data/users/johndoe/other_repos/linux",
                edenClientPath=b"/home/johndoe/.eden/clients/linux2",
                state=MountState.RUNNING,
            ),
        ]
        instance = EdenInstance(
            config_dir="/home/johndoe/.eden",
            etc_eden_dir="/etc/eden",
            home_dir="/home/johndoe",
        )
        config_checkouts = [
            EdenCheckout(
                instance,
                Path("/data/users/johndoe/mercurial"),
                Path("/home/johndoe/.eden/clients/mercurial"),
            ),
            EdenCheckout(
                instance,
                Path("/data/users/johndoe/git"),
                Path("/home/johndoe/.eden/clients/git"),
            ),
            EdenCheckout(
                instance,
                Path("/data/users/johndoe/repos/linux"),
                Path("/home/johndoe/.eden/clients/linux"),
            ),
            EdenCheckout(
                instance,
                Path("/data/users/johndoe/other_repos/linux"),
                Path("/home/johndoe/.eden/clients/linux2"),
            ),
            EdenCheckout(
                instance,
                Path("/data/users/johndoe/www"),
                Path("/home/johndoe/.eden/clients/www"),
            ),
        ]

        mounts = main_mod.ListCmd.combine_mount_info(thrift_mounts, config_checkouts)

        normal_out = TestOutput()
        main_mod.ListCmd.print_mounts(normal_out, mounts)
        self.assertEqual(
            """\
/data/users/johndoe/apache (unconfigured)
/data/users/johndoe/configs (INITIALIZING) (unconfigured)
/data/users/johndoe/git (SHUTTING_DOWN)
/data/users/johndoe/mercurial
/data/users/johndoe/other_repos/linux
/data/users/johndoe/repos/linux
/data/users/johndoe/www (not mounted)
""",
            normal_out.getvalue(),
        )

        json_out = TestOutput()
        main_mod.ListCmd.print_mounts_json(json_out, mounts)
        self.assertEqual(
            """\
{
  "/data/users/johndoe/apache": {
    "configured": false,
    "data_dir": "/home/johndoe/.eden/clients/apache",
    "state": "RUNNING"
  },
  "/data/users/johndoe/configs": {
    "configured": false,
    "data_dir": "/home/johndoe/.eden/clients/configs",
    "state": "INITIALIZING"
  },
  "/data/users/johndoe/git": {
    "configured": true,
    "data_dir": "/home/johndoe/.eden/clients/git",
    "state": "SHUTTING_DOWN"
  },
  "/data/users/johndoe/mercurial": {
    "configured": true,
    "data_dir": "/home/johndoe/.eden/clients/mercurial",
    "state": "RUNNING"
  },
  "/data/users/johndoe/other_repos/linux": {
    "configured": true,
    "data_dir": "/home/johndoe/.eden/clients/linux2",
    "state": "RUNNING"
  },
  "/data/users/johndoe/repos/linux": {
    "configured": true,
    "data_dir": "/home/johndoe/.eden/clients/linux",
    "state": "RUNNING"
  },
  "/data/users/johndoe/www": {
    "configured": true,
    "data_dir": "/home/johndoe/.eden/clients/www",
    "state": "NOT_RUNNING"
  }
}
""",
            json_out.getvalue(),
        )

    def test_list_mounts_old_thrift(self):
        self.maxDiff = None

        # Simulate an older edenfs daemon that does not send the "state" field
        thrift_mounts = [
            MountInfo(
                mountPoint=b"/data/users/johndoe/mercurial",
                edenClientPath=b"/home/johndoe/.eden/clients/mercurial",
            ),
            MountInfo(
                mountPoint=b"/data/users/johndoe/git",
                edenClientPath=b"/home/johndoe/.eden/clients/git",
            ),
            MountInfo(
                mountPoint=b"/data/users/johndoe/configs",
                edenClientPath=b"/home/johndoe/.eden/clients/configs",
            ),
        ]
        instance = EdenInstance(
            config_dir="/home/johndoe/.eden",
            etc_eden_dir="/etc/eden",
            home_dir="/home/johndoe",
        )
        config_checkouts = [
            EdenCheckout(
                instance,
                Path("/data/users/johndoe/mercurial"),
                Path("/home/johndoe/.eden/clients/mercurial"),
            ),
            EdenCheckout(
                instance,
                Path("/data/users/johndoe/git"),
                Path("/home/johndoe/.eden/clients/git"),
            ),
            EdenCheckout(
                instance,
                Path("/data/users/johndoe/www"),
                Path("/home/johndoe/.eden/clients/www"),
            ),
        ]

        mounts = main_mod.ListCmd.combine_mount_info(thrift_mounts, config_checkouts)

        normal_out = TestOutput()
        main_mod.ListCmd.print_mounts(normal_out, mounts)
        self.assertEqual(
            """\
/data/users/johndoe/configs (unconfigured)
/data/users/johndoe/git
/data/users/johndoe/mercurial
/data/users/johndoe/www (not mounted)
""",
            normal_out.getvalue(),
        )

        json_out = TestOutput()
        main_mod.ListCmd.print_mounts_json(json_out, mounts)
        self.assertEqual(
            """\
{
  "/data/users/johndoe/configs": {
    "configured": false,
    "data_dir": "/home/johndoe/.eden/clients/configs",
    "state": "RUNNING"
  },
  "/data/users/johndoe/git": {
    "configured": true,
    "data_dir": "/home/johndoe/.eden/clients/git",
    "state": "RUNNING"
  },
  "/data/users/johndoe/mercurial": {
    "configured": true,
    "data_dir": "/home/johndoe/.eden/clients/mercurial",
    "state": "RUNNING"
  },
  "/data/users/johndoe/www": {
    "configured": true,
    "data_dir": "/home/johndoe/.eden/clients/www",
    "state": "NOT_RUNNING"
  }
}
""",
            json_out.getvalue(),
        )
