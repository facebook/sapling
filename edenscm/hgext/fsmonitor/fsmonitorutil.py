# util.py - fsmonitor utilities
#
# Copyright 2013-2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


def shortlist(listlike, count=None, limit=4):
    """Return a value that can be converted to Rust blackbox::event::ShortList"""
    shortlist = []
    for name in listlike:
        shortlist.append(name)
        if len(shortlist) > limit:
            break
    if count is None:
        count = len(listlike)
    return {"short_list": shortlist, "len": count}
