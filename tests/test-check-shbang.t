#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "`dirname "$TESTDIR"`"

look for python scripts that do not use /usr/bin/env

  $ hg files 'set:grep(r"^#!.*?python") and not grep(r"^#!/usr/bi{1}n/env python")'
  [1]

look for shell scripts that do not use /bin/sh

  $ hg files 'set:grep(r"^#!.*/bi{1}n/sh") and not grep(r"^#!/bi{1}n/sh")'
  [1]
