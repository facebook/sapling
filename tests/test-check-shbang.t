#require test-repo hg10 slow

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "`dirname "$TESTDIR"`"

look for python scripts that do not use /usr/bin/env

  $ testrepohg files 'set:** and grep(r"^#!.*?python") and not grep(r"^#!/usr/bi{1}n/env python") - **/*.t'

In tests, enforce $PYTHON and *not* /usr/bin/env python or similar:
  $ testrepohg files 'set:**/*.t and grep(r"#!.*?python")' \
  > -X tests/test-check-execute.t \
  > -X tests/test-check-module-imports.t \
  > -X tests/test-check-pyflakes.t \
  > -X tests/test-check-shbang.t \
  > -X fb-hgext/tests/test-fb-hgext-check-execute-hg.t \
  > -X fb-hgext/tests/test-fb-hgext-check-pyflakes-hg.t \
  > -X fb-hgext/tests/test-fb-hgext-check-shbang-hg.t
  [1]

The above exclusions are because they're looking for files that
contain Python but don't end in .py - please avoid adding more.

look for shell scripts that do not use /bin/sh

  $ testrepohg files 'set:** and grep(r"^#!.*/bi{1}n/sh") and not grep(r"^#!/bi{1}n/sh")'
  [1]
