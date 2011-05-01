#!/usr/bin/env python

# Filter output by pyflakes to control which warnings we check

import sys, re

def makekey(message):
    # "path/file:line: message"
    match = re.search(r"(line \d+)", message)
    line = ''
    if match:
        line = match.group(0)
        message = re.sub(r"(line \d+)", '', message)
    return re.sub(r"([^:]*):([^:]+):([^']*)('[^']*')(.*)$",
                  r'\3:\5:\4:\1:\2:' + line,
                  message)

lines = []
for line in sys.stdin:
    # We whitelist tests
    if not re.search("imported but unused", line):
        continue
    lines.append(line)

for line in sorted(lines, key = makekey):
    sys.stdout.write(line)
print
