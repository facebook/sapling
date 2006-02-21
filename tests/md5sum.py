#! /usr/bin/env python
import sys
import os
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
