#!/usr/bin/env python

# Filter output by pyflakes to control which warnings we check

import sys, re, os

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
    pats = [
            r"imported but unused",
            r"local variable '.*' is assigned to but never used",
            r"unable to detect undefined names",
           ]
    if not re.search('|'.join(pats), line):
        continue
    fn = line.split(':', 1)[0]
    f = open(os.path.join(os.path.dirname(os.path.dirname(__file__)), fn))
    data = f.read()
    f.close()
    if 'no-check-code' in data:
        continue
    lines.append(line)

for line in sorted(lines, key = makekey):
    sys.stdout.write(line)
print
