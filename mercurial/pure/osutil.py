# osutil.py - pure Python version of osutil.c
#
#  Copyright 2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os
import stat as statmod

posixfile = open

def _mode_to_kind(mode):
    if statmod.S_ISREG(mode):
        return statmod.S_IFREG
    if statmod.S_ISDIR(mode):
        return statmod.S_IFDIR
    if statmod.S_ISLNK(mode):
        return statmod.S_IFLNK
    if statmod.S_ISBLK(mode):
        return statmod.S_IFBLK
    if statmod.S_ISCHR(mode):
        return statmod.S_IFCHR
    if statmod.S_ISFIFO(mode):
        return statmod.S_IFIFO
    if statmod.S_ISSOCK(mode):
        return statmod.S_IFSOCK
    return mode

def listdir(path, stat=False, skip=None):
    '''listdir(path, stat=False) -> list_of_tuples

    Return a sorted list containing information about the entries
    in the directory.

    If stat is True, each element is a 3-tuple:

      (name, type, stat object)

    Otherwise, each element is a 2-tuple:

      (name, type)
    '''
    result = []
    prefix = path
    if not prefix.endswith(os.sep):
        prefix += os.sep
    names = os.listdir(path)
    names.sort()
    for fn in names:
        st = os.lstat(prefix + fn)
        if fn == skip and statmod.S_ISDIR(st.st_mode):
            return []
        if stat:
            result.append((fn, _mode_to_kind(st.st_mode), st))
        else:
            result.append((fn, _mode_to_kind(st.st_mode)))
    return result

