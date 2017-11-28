#!/usr/bin/env python
from __future__ import absolute_import, print_function

from glob import glob

import os
import re
import sys

"""
Check if:
  - a test is using foreign extensions without proper checks
  - a test is using an extension in this repo without absolute path
"""

# whitelisted foreign extensions
foreignexts = set(['remotenames'])
foreignextre = re.compile(r'(%s)' % '|'.join(foreignexts))

# pattern for rust extensions
rustextre = re.compile(r'^hgext3rd\.rust\..*')

# extensions in this repo
repoexts = set(os.path.basename(p).split('.')[0]
               for p in glob('hgext3rd/*.py') if '__' not in p)
repoexts.update(os.path.basename(p).split('.')[0]
                for p in glob('hgext3rd/*.pyx'))
repoexts.update(os.path.basename(os.path.dirname(p))
                for p in glob('hgext3rd/*/__init__.py'))
repoexts.update(os.path.basename(os.path.dirname(p))
                for p in glob('*/__init__.py'))
repoextre = re.compile(r'(%s)' % '|'.join(repoexts))

checkres = [
    (re.compile(r'^\s*>\s*%s\s*=\s*$' % foreignextre.pattern),
     'use "$ . $TESTDIR/require-ext.sh %(name)s" to skip the test'
     ' if %(name)s is not available'),
    (re.compile(r'^\s*\$.*--config[ =\']*extensions.%s='
                % foreignextre.pattern),
     'use "$ . $TESTDIR/require-ext.sh %(name)s" to skip the test'
     ' if %(name)s is not available'),
    (re.compile(r'^\s*>\s*%s\s*=\s*$' % repoextre.pattern),
     'use full path like $TESTDIR/../hgext3rd/%(name)s.py for extension '
     'in this repo'),
    (re.compile(r'^\s*\$.*--config[ =\']*extensions.%s=[ \'"]'
                % repoextre.pattern),
     'use full path like $TESTDIR/../hgext3rd/%(name)s.py for extension '
     'in this repo'),
]

requirere = re.compile(r'require-ext\.sh (.*)$')

def checkfile(path):
    errors = []
    with open(path) as f:
        required = set()
        for i, line in enumerate(f):
            msg = None
            m = requirere.search(line)
            if m:
                requiredexts = set(m.group(1).split())
                unknownexts = requiredexts - foreignexts
                for e in unknownexts:
                    if rustextre.match(e):
                        continue
                    if e in repoexts:
                        msg = 'do not require non-foreign extension %s'
                    else:
                        # change foreignexts if this error is a false postive
                        msg = 'do not require non-whitelisted extension %s'
                    errors.append((path, i + 1, line[:-1], msg % e))
                required.update(requiredexts)
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
