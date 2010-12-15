#!/usr/bin/env python

# Filter output by the progress extension to make it readable in tests

import sys, re

for line in sys.stdin:
    line = re.sub(r'\r+[^\n]', lambda m: '\n' + m.group()[-1:], line)
    sys.stdout.write(line)
print
