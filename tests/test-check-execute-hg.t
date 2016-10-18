#require test-repo execbit

  $ . $TESTDIR/require-core-hg.sh contrib/check-config.py

This file is backported from mercurial/tests/test-check-execute.t.
It differs slightly to fix paths.

  $ . "$RUNTESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

look for python scripts without the execute bit

  $ hg files 'set:**.py and not exec() and grep(r"^#!.*?python")'
  [1]

look for python scripts with execute bit but not shebang

  $ hg files 'set:**.py and exec() and not grep(r"^#!.*?python")'
  [1]

look for shell scripts with execute bit but not shebang

  $ hg files 'set:**.sh and exec() and not grep(r"^#!.*(ba)?sh")'
  [1]

look for non scripts with no shebang

  $ hg files 'set:exec() and not **.sh and not **.py and not grep(r"^#!")'
  [1]
