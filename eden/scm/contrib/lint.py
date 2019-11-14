#!/usr/bin/env python
# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

""" Convert mercurial check-code errors into a format
that plays nicely with arc lint """
from __future__ import absolute_import, print_function

import errno
import os
import re
import subprocess
import sys

import utils


sys.path.insert(0, os.path.dirname(__file__))

# Normalize the list of files that we should report on
wanted = set()
for path in sys.argv[1:]:
    wanted.add(os.path.relpath(path))

# Export LINTFILES so tests can skip unrelated files
if wanted:
    os.environ["LINTFILES"] = "\n".join(sorted(wanted))

args = ["-l", "test-check-code-hg.t", "test-check-pyflakes-hg.t"]

try:
    proc = utils.spawnruntests(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
except OSError as ex:
    if ex.errno == errno.ENOENT:
        print(
            "lint.py:1: ERROR:ENVIRON: Please either set "
            + "MERCURIALRUNTEST var to the full path to run-tests.py, "
            + "or add the containing directory to your $PATH"
        )
    else:
        print("lint.py:1: ERROR:OSError: %r" % ex)
    sys.exit(0)

output, error = proc.communicate()

context_file = None
lines = error.split("\n")
# We expect a run of 3 lines to describe the error, with the first
# of those to look like a filename and line number location
while lines:
    line = lines[0]
    lines.pop(0)

    # test-check-pyflakes-hg style output
    m = re.match("^\+  ([a-zA-Z0-9_./-]+):(\d+): (.*)$", line)
    if m:
        filename, location, why = m.groups()
        if filename in wanted:
            print("%s:%s: ERROR:Pyflakes: %s" % (filename, location, why))
        continue

    # test-check-code-hg style output
    if re.match("^--- (.*)$", line):
        context_file = os.path.relpath(line[4:])
        continue

    m = re.match("^\+  (Skipping (.*) it has no.*)$", line)
    if m:
        filename = m.group(2)
        if filename in wanted:
            print(
                "%s:0: ERROR:CheckCode: Update %s to add %s"
                % (filename, context_file, m.group(1))
            )
        continue

    if not re.match("^\+ +[a-zA-Z0-9_./-]+:\d+:$", line):
        continue

    if len(lines) < 2:
        continue

    location = line
    context = lines.pop(0)  # we ignore this
    why = lines.pop(0)

    location = location[1:].strip()  # strip off the "+  " bit
    location = location.rstrip(":")
    filename, lineno = location.split(":")

    if filename not in wanted:
        # lint doesn't care about this file.
        continue

    why = why[1:].strip()

    print("%s: ERROR:CheckCode: %s" % (location, why))
