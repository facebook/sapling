#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import os.path
import pathlib
import unittest

from .lib.linux import LinuxCgroup, is_cgroup_v2_mounted


class LinuxCgroupTest(unittest.TestCase):
    def test_parse_proc_file(self) -> None:
        proc_file_content = (
            b"0::/user.slice/user-6986.slice/session-33.scope/init.scope\n"
        )
        self.assertEqual(
            LinuxCgroup._parse_proc_file(proc_file_content),
            b"/user.slice/user-6986.slice/session-33.scope/init.scope",
        )

    def test_parsing_empty_proc_file_fails(self) -> None:
        with self.assertRaises(ValueError):
            LinuxCgroup._parse_proc_file(b"")

        with self.assertRaises(ValueError):
            LinuxCgroup._parse_proc_file(b"\n")

    def test_parsing_proc_file_with_multiple_cgroups_v1_hierarchies_fails(self) -> None:
        proc_file_content = (
            b"12:cpuacct:/user.slice/user-2233.slice/session-163872.scope\n"
            b"11:freezer:/\n"
            b"10:hugetlb:/\n"
            b"9:blkio:/user.slice/user-2233.slice/session-163872.scope\n"
            b"8:cpuset:/\n"
            b"7:pids:/user.slice/user-2233.slice/session-163872.scope\n"
            b"6:devices:/user.slice\n"
            b"5:memory:/user.slice/user-2233.slice/session-163872.scope\n"
            b"4:perf_event:/\n"
            b"3:net_cls,net_prio:/\n"
            b"2:cpu:/user.slice/user-2233.slice/session-163872.scope\n"
            b"1:name=systemd:/user.slice/user-2233.slice/session-163872.scope\n"
        )
        with self.assertRaises(NotImplementedError):
            LinuxCgroup._parse_proc_file(proc_file_content)

    def test_cgroup_from_sys_fs_cgroup_path(self) -> None:
        path = pathlib.PosixPath("/sys/fs/cgroup/system.slice")
        cgroup = LinuxCgroup.from_sys_fs_cgroup_path(path)
        self.assertEqual(cgroup.name, b"/system.slice")

    def test_sys_fs_cgroup_path(self) -> None:
        cgroup = LinuxCgroup(b"/user.slice/user-6986.slice/session-13.scope/init.scope")
        self.assertEqual(
            cgroup.sys_fs_cgroup_path,
            pathlib.PosixPath(
                "/sys/fs/cgroup//user.slice/user-6986.slice/session-13.scope/init.scope"
            ),
        )

    @unittest.skipIf(
        not is_cgroup_v2_mounted(),
        "T36934106: Fix EdenFS systemd integration tests for cgroups v1",
    )
    def test_cgroup_from_current_process_includes_current_process_id(self) -> None:
        cgroup = LinuxCgroup.from_current_process()
        self.assertIn(os.getpid(), cgroup.query_process_ids())
