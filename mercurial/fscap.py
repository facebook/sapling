# Copyright Facebook, Inc. 2018
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""capabilities of well-known filesystems"""

SYMLINK = "symlink"
HARDLINK = "hardlink"
EXECBIT = "execbit"

_ALL_CAPS = {SYMLINK: True, HARDLINK: True, EXECBIT: True}

_FS_CAP_TABLE = {
    "apfs": _ALL_CAPS,
    "btrfs": _ALL_CAPS,
    "eden": {SYMLINK: True, HARDLINK: False, EXECBIT: True},
    "ext2": _ALL_CAPS,
    "ext3": _ALL_CAPS,
    "ext4": _ALL_CAPS,
    "hfs": _ALL_CAPS,
    "jfs": _ALL_CAPS,
    "reiserfs": _ALL_CAPS,
    "tmpfs": _ALL_CAPS,
    "ufs": _ALL_CAPS,
    "xfs": _ALL_CAPS,
    "zfs": _ALL_CAPS,
}


def getfscap(fstype, cap):
    """Test if a filesystem has specified capability.

    Return True if it has, False if it doesn't, or None if unsure.
    """
    return _FS_CAP_TABLE.get(fstype, {}).get(cap)
