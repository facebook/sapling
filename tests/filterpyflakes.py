#!/usr/bin/env python

# Filter output by pyflakes to control which warnings we check

import sys, re, os

def makekey(typeandline):
    """
    for sorting lines by: msgtype, path/to/file, lineno, message

    typeandline is a sequence of a message type and the entire message line
    the message line format is path/to/file:line: message

    >>> makekey((3, 'example.py:36: any message'))
    (3, 'example.py', 36, ' any message')
    >>> makekey((7, 'path/to/file.py:68: dummy message'))
    (7, 'path/to/file.py', 68, ' dummy message')
    >>> makekey((2, 'fn:88: m')) > makekey((2, 'fn:9: m'))
    True
    """

    msgtype, line = typeandline
    fname, line, message = line.split(":", 2)
    # line as int for ordering 9 before 88
    return msgtype, fname, int(line), message


lines = []
for line in sys.stdin:
    # We whitelist tests (see more messages in pyflakes.messages)
    pats = [
            (r"imported but unused", None),
            (r"local variable '.*' is assigned to but never used", None),
            (r"unable to detect undefined names", None),
            (r"undefined name '.*'",
             r"undefined name '(WindowsError|memoryview)'")
           ]

    for msgtype, (pat, excl) in enumerate(pats):
        if re.search(pat, line) and (not excl or not re.search(excl, line)):
            break # pattern matches
    else:
        continue # no pattern matched, next line
    fn = line.split(':', 1)[0]
    f = open(os.path.join(os.path.dirname(os.path.dirname(__file__)), fn))
    data = f.read()
    f.close()
    if 'no-' 'check-code' in data:
        continue
    lines.append((msgtype, line))

for msgtype, line in sorted(lines, key=makekey):
    sys.stdout.write(line)
print

# self test of "undefined name" detection for other than 'memoryview'
if False:
    print undefinedname
