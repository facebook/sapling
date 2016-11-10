#!/usr/bin/env python

# Filter output by pyflakes to control which warnings we check

from __future__ import absolute_import, print_function

import re
import sys

lines = []
for line in sys.stdin:
    # We blacklist tests that are too noisy for us
    pats = [
        r"undefined name '(WindowsError|memoryview)'",
        r"redefinition of unused '[^']+' from line",
    ]

    keep = True
    for pat in pats:
        if re.search(pat, line):
            keep = False
            break # pattern matches
    if keep:
        fn = line.split(':', 1)[0]
        f = open(fn)
        data = f.read()
        f.close()
        if 'no-' 'check-code' in data:
            continue
        lines.append(line)

for line in lines:
    sys.stdout.write(line)
print()

# self test of "undefined name" detection for other than 'memoryview'
if False:
    print(memoryview)
    print(undefinedname)
