# util.py - fsmonitor utilities
#
# Copyright 2013-2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


def reprshort(filelist, limit=20):
    """Like repr(filelist). But truncate it if it is too long"""
    if len(filelist) <= limit:
        return repr(filelist)
    else:
        return "%r and %s more entries" % (filelist[:limit], len(filelist) - limit)
