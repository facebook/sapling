#!/usr/bin/env python
import errno
import os
import subprocess
import re
import sys

""" Convert mercurial check-code errors into a format
that plays nicely with arc lint """

# Normalize the list of files that we should report on
wanted = set()
for path in sys.argv[1:]:
    wanted.add(os.path.relpath(path))

try:
    args = ['./run-tests.py', '-j8', 'test-check-code-hg.t']

    # Check lz4revlog requirement
    reporoot = os.path.join(os.path.dirname(os.path.dirname(__file__)), '.hg')
    with open(os.path.join(reporoot, 'requires'), 'r') as f:
        if 'lz4revlog\n' in f:
            args.append('--extra-config-opt=extensions.lz4revlog=')

    proc = subprocess.Popen(args, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, cwd='tests')
except OSError as ex:
    if ex.errno == errno.ENOENT:
        print 'lint.py:1: ERROR:ENVIRON: Please either set ' + \
              'MERCURIALRUNTEST var to the full path to run-tests.py, ' + \
              'or add the containing directory to your $PATH'
    else:
        print 'lint.py:1: ERROR:OSError: %s: %s' % (runner, str(ex))
    sys.exit(0)

output, error = proc.communicate()

lines = error.split('\n')
# We expect a run of 3 lines to describe the error, with the first
# of those to look like a filename and line number location
while len(lines) >= 3:
    line = lines[0]
    if not re.match('^\+ +[a-zA-Z0-9_./-]+:\d+:$', line):
        lines.pop(0)
        continue
    location = lines[0]
    context = lines[1]  # we ignore this
    why = lines[2]

    location = location[1:].strip() # strip off the "+  " bit
    location = location.rstrip(':')
    filename, lineno = location.split(':')
    # Consume those 3 lines
    lines = lines[3:]

    if filename not in wanted:
        # lint doesn't care about this file.
        continue

    why = why[1:].strip()

    print '%s: ERROR:CheckCode: %s' % (location, why)

