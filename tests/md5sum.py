#!/usr/bin/env python
#
# Based on python's Tools/scripts/md5sum.py
#
# This software may be used and distributed according to the terms
# of the PYTHON SOFTWARE FOUNDATION LICENSE VERSION 2, which is
# GPL-compatible.

import sys
import md5

for filename in sys.argv[1:]:
    try:
        fp = open(filename, 'rb')
    except IOError, msg:
        sys.stderr.write('%s: Can\'t open: %s\n' % (filename, msg))
        sys.exit(1)

    m = md5.new()
    try:
        while 1:
            data = fp.read(8192)
            if not data:
                break
            m.update(data)
    except IOError, msg:
        sys.stderr.write('%s: I/O error: %s\n' % (filename, msg))
        sys.exit(1)
    sys.stdout.write('%s  %s\n' % (m.hexdigest(), filename))

sys.exit(0)
