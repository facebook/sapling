#!/usr/bin/env python
from __future__ import absolute_import, print_function

import re
import sys

"""
Check if a test is using foreign extensions without proper checks
"""

# foreign extensions
exts = ['directaccess', 'evolve', 'inhibit', 'remotenames']
extre = re.compile(r'(%s)' % '|'.join(exts))

checkres = [
    (re.compile(r'^\s*>\s*%s\s*=\s*$' % extre.pattern),
     'use "$ . $TESTDIR/require-ext.sh %(name)s" to skip the test'
     ' if %(name)s is not available'),
    (re.compile(r'^\s*\$.*--config[ =\']*extensions.%s=' % extre.pattern),
     'use "$ . $TESTDIR/require-ext.sh %(name)s" to skip the test'
     ' if %(name)s is not available'),
]

# $ . $TESTDIR/require-ext.sh foreignext
requirere = re.compile(r'require-ext\.sh ((?:%s|\s+)+)' % extre.pattern)

def checkfile(path):
    errors = []
    with open(path) as f:
        required = set()
        for i, line in enumerate(f):
            msg = None
            m = requirere.search(line)
            if m:
                required.update(m.group(1).split())
            for regex, msg in checkres:
                m = regex.search(line)
                if not m:
                    continue
                name = m.group(1)
                if name in required:
                    continue
                # line[:-1] is to remove the last "\n"
                errors.append((path, i + 1, line[:-1], msg % {'name': name}))
                # only one error per extension per file
                required.add(name)
    return errors

def checkfiles(paths):
    errors = []
    for path in sys.argv[1:]:
        errors += checkfile(path)
    return sorted(set(errors))

def printerrors(errors):
    # same format with check-code.py
    for fname, lineno, line, msg in errors:
        print('%s:%d:\n > %s\n %s' % (fname, lineno, line, msg))

printerrors(checkfiles(sys.argv[1:]))
