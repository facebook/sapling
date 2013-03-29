# dicthelpers.py - helper routines for Python dicts
#
# Copyright 2013 Facebook
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

def _diffjoin(d1, d2, default, compare):
    res = {}
    if d1 is d2 and compare:
        # same dict, so diff is empty
        return res

    for k1, v1 in d1.iteritems():
        if k1 in d2:
            v2 = d2[k1]
            if not compare or v1 != v2:
                res[k1] = (v1, v2)
        else:
            res[k1] = (v1, default)

    if d1 is d2:
        return res

    for k2 in d2:
        if k2 not in d1:
            res[k2] = (default, d2[k2])

    return res

def diff(d1, d2, default=None):
    '''Return all key-value pairs that are different between d1 and d2.

    This includes keys that are present in one dict but not the other, and
    keys whose values are different. The return value is a dict with values
    being pairs of values from d1 and d2 respectively, and missing values
    represented as default.'''
    return _diffjoin(d1, d2, default, True)

def join(d1, d2, default=None):
    '''Return all key-value pairs from both d1 and d2.

    This is akin to an outer join in relational algebra. The return value is a
    dict with values being pairs of values from d1 and d2 respectively, and
    missing values represented as default.'''
    return _diffjoin(d1, d2, default, False)
