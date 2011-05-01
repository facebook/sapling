#!/usr/bin/env python

# Filter output by pyflakes to control which warnings we check

import sys, re

for line in sys.stdin:
    # We whitelist tests
    if not re.search("imported but unused", line):
        continue
    sys.stdout.write(line)
print
