# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


def bisect(l, r, comp, val):
    """Bisect algorithm with custom compare function

    Returns smallest index between l and r whose value is equal to val.
    Returns None if there are no such index.
    """
    if r < l:
        return None
    while l < r:
        m = (l + r) / 2
        cmpresult = comp(m, val)
        if cmpresult == -1:
            l = m + 1
        elif cmpresult == 0:
            r = m
        else:
            r = m - 1

    if r < l or comp(l, val) != 0:
        return None
    return l
