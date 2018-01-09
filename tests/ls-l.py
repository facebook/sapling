#!/usr/bin/env python

# like ls -l, but do not print date, user, or non-common mode bit, to avoid
# using globs in tests.
from __future__ import absolute_import, print_function

import os
import stat
import sys

def modestr(st):
    mode = st.st_mode
    result = ''
    if mode & stat.S_IFDIR:
        result += 'd'
    else:
        result += '-'
    for owner in ['USR', 'GRP', 'OTH']:
        for action in ['R', 'W', 'X']:
            if mode & getattr(stat, 'S_I%s%s' % (action, owner)):
                result += action.lower()
            else:
                result += '-'
    return result

def sizestr(st):
    if st.st_mode & stat.S_IFREG:
        return '%7d' % st.st_size
    else:
        # do not show size for non regular files
        return ' ' * 7

os.chdir((sys.argv[1:] + ['.'])[0])

for name in sorted(os.listdir('.')):
    st = os.stat(name)
    print('%s %s %s' % (modestr(st), sizestr(st), name))
