#!/usr/bin/env python
import errno
import os
import subprocess
import re
import sys

""" Convert mercurial check-code errors into a format
that plays nicely with arc lint """

runner = os.environ.get('MERCURIALRUNTEST', 'run-tests.py')

if not os.path.exists(runner):
    # If it looks like we're in facebook-hg-rpms, let's try
    # running against the associated hg-crew tests
    # otherwise, Popen will search for run-tests.py in the PATH
    default_runner = os.path.relpath('../hg-crew/tests/run-tests.py')
    if os.path.exists(default_runner):
        runner = os.path.abspath(default_runner)

# Normalize the list of files that we should report on
wanted = set()
for path in sys.argv[1:]:
    wanted.add(os.path.relpath(path))

try:
    args = [runner, '-j2', '-l',
            'test-check-code-hg.t',
            'test-check-pyflakes-hg.t']

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

context_file = None
lines = error.split('\n')
# We expect a run of 3 lines to describe the error, with the first
# of those to look like a filename and line number location
while lines:
    line = lines[0]
    lines.pop(0)

    # test-check-pyflakes-hg style output
    m = re.match('^\+  ([a-zA-Z0-9_./-]+):(\d+): (.*)$', line)
    if m:
        filename, location, why = m.groups()
        if filename in wanted:
            print '%s:%s: ERROR:Pyflakes: %s' % (filename, location, why)
        continue

    # test-check-code-hg style output
    if re.match('^--- (.*)$', line):
        context_file = os.path.relpath(line[4:])
        continue

    m = re.match('^\+  (Skipping (.*) it has no.*)$', line)
    if m:
        filename = m.group(2)
        if filename in wanted:
            print '%s:0: ERROR:CheckCode: Update %s to add %s' % (
                filename, context_file, m.group(1))
        continue

    if not re.match('^\+ +[a-zA-Z0-9_./-]+:\d+:$', line):
        continue

    if len(lines) < 2:
        continue

    location = line
    context = lines.pop(0) # we ignore this
    why = lines.pop(0)

    location = location[1:].strip() # strip off the "+  " bit
    location = location.rstrip(':')
    filename, lineno = location.split(':')

    if filename not in wanted:
        # lint doesn't care about this file.
        continue

    why = why[1:].strip()

    print '%s: ERROR:CheckCode: %s' % (location, why)

