#!/usr/bin/env python
#
# Based on python's Tools/scripts/md5sum.py
#
# This software may be used and distributed according to the terms
# of the PYTHON SOFTWARE FOUNDATION LICENSE VERSION 2, which is
# GPL-compatible.

from __future__ import absolute_import

import hashlib
import os
import sys

try:
    import msvcrt
    msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
    msvcrt.setmode(sys.stderr.fileno(), os.O_BINARY)
except ImportError:
    pass

for filename in sys.argv[1:]:
    try:
        fp = open(filename, 'rb')
    except IOError as msg:
        sys.stderr.write('%s: Can\'t open: %s\n' % (filename, msg))
        sys.exit(1)

    m = hashlib.md5()
    try:
        for data in iter(lambda: fp.read(8192), b''):
            m.update(data)
    except IOError as msg:
        sys.stderr.write('%s: I/O error: %s\n' % (filename, msg))
        sys.exit(1)
    sys.stdout.write('%s  %s\n' % (m.hexdigest(), filename))

sys.exit(0)
