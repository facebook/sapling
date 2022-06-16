#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import abc
import errno
import logging
import multiprocessing
import os
import random
import re
import subprocess
import sys
from typing import List, NamedTuple, Union


log: logging.Logger = logging.getLogger("eden.fs.cli.mtab")


MountInfo = NamedTuple(
    "MountInfo", [("device", bytes), ("mount_point", bytes), ("vfstype", bytes)]
)


MTStat = NamedTuple("MTStat", [("st_uid", int), ("st_dev", int), ("st_mode", int)])

kMountStaleSecondsTimeout = 10

# Note this function needs to be a global function, otherwise it will cause
# errors to spawn a process with this function as the entry point.
def lstat_process(path: Union[bytes, str]) -> None:
    """
    Function to be the entry point of the multiproccessing process
    to stat path. stat might hang so we might kill this process.
    The return code of this process is the exit code of lstat.
    """
    try:
        os.lstat(path)
    except OSError as e:
        exit(e.errno)
    exit(0)


class MountTable(abc.ABC):
    @abc.abstractmethod
    def read(self) -> List[MountInfo]:
        "Returns the list of system mounts."

    @abc.abstractmethod
    def unmount_lazy(self, mount_point: bytes) -> bool:
        "Corresponds to `umount -l` on Linux."

    @abc.abstractmethod
    def unmount_force(self, mount_point: bytes) -> bool:
        "Corresponds to `umount -f` on Linux."

    def lstat(self, path: Union[bytes, str]) -> MTStat:
        "Returns a subset of the results of os.lstat."
        st = os.lstat(path)
        return MTStat(st_uid=st.st_uid, st_dev=st.st_dev, st_mode=st.st_mode)

    def check_path_access(self, path: bytes, mount_type: bytes) -> None:
        """\
        Attempts to stat the given directory, bypassing the kernel's caches.
        Raises OSError upon failure.
        """
        # For FUSE it's pretty easy, we can lstat the mount. ENOENT means the
        # mount seems to be working fine and ENOTCONN means the mount is stale.
        if mount_type == b"fuse":
            try:
                # Even if the FUSE process is shut down, the lstat call will succeed if
                # the stat result is cached. Append a random string to avoid that. In a
                # better world, this code would bypass the cache by opening a handle
                # with O_DIRECT, but EdenFS does not support O_DIRECT.
                os.lstat(os.path.join(path, hex(random.getrandbits(32))[2:].encode()))
            except OSError as e:
                if e.errno == errno.ENOENT:
                    return
                raise
        # For NFS it's less easy. Stating the mount point will hang if the mount
        # is stale. So we need to add a timeout on our stat call. A timeout
        # means the mount is stale and ENOENT still means the mount seems to be
        # working properly.
        elif mount_type == b"nfs":
            proc = multiprocessing.Process(
                target=lstat_process,
                args=(os.path.join(path, hex(random.getrandbits(32))[2:].encode()),),
            )
            proc.start()
            proc.join(timeout=kMountStaleSecondsTimeout)
            if proc.is_alive():
                # ask the lstat to terminate nicely, this is expected to succeed.
                proc.terminate()
                # note we need a timeout here incase the process is miss
                # behaving and refuses to exit
                proc.join(timeout=kMountStaleSecondsTimeout)
                # if terminate didn't work then we fallback to killing the process
                if proc.is_alive():
                    proc.kill()
                    # note we need a timeout here incase the process blocked on
                    # an uniteruptable syscall and refuses to exit (should not
                    # be the case, but ya know ... caution)
                    proc.join(timeout=kMountStaleSecondsTimeout)
                # if the process is alive at this point the only thing that we
                # can hope to kill it is the umount that we will trigger later.
                # But we can still close all the resources the process might be
                # holding on to to prevent deadlocks and such.
                if proc.is_alive():
                    proc.close()

                raise OSError(
                    errno.ENOTCONN,
                    "Stating the mount timed out, mount point is not connected",
                )
            else:
                if proc.exitcode == errno.ENOENT:
                    return
                # The return code is technically an optional, but it should
                # only be none if the process has not yet terminated.
                if proc.exitcode is None:
                    raise Exception(
                        """
Reaching here should be impossible, checking process was terminated, but does
not seem to have finished.
"""
                    )
                raise OSError(proc.exitcode, "stating the mountpoint failed")
        raise Exception(f"Unknown mount type {mount_type}")

    @abc.abstractmethod
    def create_bind_mount(self, source_path, dest_path) -> bool:
        "Creates a bind mount from source_path to dest_path."


def parse_mtab(contents: bytes) -> List[MountInfo]:
    mounts = []
    for line in contents.splitlines():
        # columns split by space or tab per man page
        entries = line.split()
        if len(entries) != 6:
            log.warning(f"mount table line has {len(entries)} entries instead of 6")
            continue
        device, mount_point, vfstype, opts, freq, passno = entries
        mounts.append(
            MountInfo(device=device, mount_point=mount_point, vfstype=vfstype)
        )
    return mounts


class LinuxMountTable(MountTable):
    def read(self) -> List[MountInfo]:
        # What's the most portable mtab path? I've seen both /etc/mtab and
        # /proc/self/mounts.  CentOS 6 in particular does not symlink /etc/mtab
        # to /proc/self/mounts so go directly to /proc/self/mounts.
        # This code could eventually fall back to /proc/mounts and /etc/mtab.
        with open("/proc/self/mounts", "rb") as f:
            return parse_mtab(f.read())

    def unmount_lazy(self, mount_point: bytes) -> bool:
        # MNT_DETACH
        return 0 == subprocess.call(["sudo", "umount", "-l", mount_point])

    def unmount_force(self, mount_point: bytes) -> bool:
        # MNT_FORCE
        return 0 == subprocess.call(["sudo", "umount", "-f", mount_point])

    def create_bind_mount(self, source_path, dest_path) -> bool:
        return 0 == subprocess.check_call(
            ["sudo", "mount", "-o", "bind", source_path, dest_path]
        )


def parse_macos_mount_output(contents: bytes) -> List[MountInfo]:
    mounts = []
    for line in contents.splitlines():
        m = re.match(b"^(\\S+) on (.+) \\(([^,]+),.*\\)$", line)
        if m:
            mounts.append(
                MountInfo(device=m.group(1), mount_point=m.group(2), vfstype=m.group(3))
            )
    return mounts


class MacOSMountTable(MountTable):
    def read(self) -> List[MountInfo]:
        # Specifying the path is important, as sudo may have munged the path
        # such that /sbin is not part of it any longer
        contents = subprocess.check_output(["/sbin/mount"])
        return parse_macos_mount_output(contents)

    def unmount_lazy(self, mount_point: bytes) -> bool:
        return False

    def unmount_force(self, mount_point: bytes) -> bool:
        return 0 == subprocess.call(["sudo", "umount", "-f", mount_point])

    def create_bind_mount(self, source_path, dest_path) -> bool:
        return False


class NopMountTable(MountTable):
    def read(self) -> List[MountInfo]:
        return []

    def unmount_lazy(self, mount_point: bytes) -> bool:
        return False

    def unmount_force(self, mount_point: bytes) -> bool:
        return False

    def create_bind_mount(self, source_path, dest_path) -> bool:
        return False


def new() -> MountTable:
    if "linux" in sys.platform:
        return LinuxMountTable()
    if sys.platform == "darwin":
        return MacOSMountTable()
    return NopMountTable()
