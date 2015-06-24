#!/usr/bin/env python

import errno, os, sys

for f in sys.argv[1:]:
    try:
        print f, '->', os.readlink(f)
    except OSError as err:
        if err.errno != errno.EINVAL:
            raise
        print f, 'not a symlink'

sys.exit(0)
