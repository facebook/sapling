#require test-repo

  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.

  $ hg files "set:(**.py or **.txt) - tests/**" | sed 's|\\|/|g' |
  >   xargs python contrib/check-config.py
  undocumented: convert.cvsps.cache (bool) [True]
  undocumented: convert.cvsps.fuzz (str) [60]
  undocumented: convert.cvsps.mergefrom (str)
  undocumented: convert.cvsps.mergeto (str)
  undocumented: convert.git.remoteprefix (str) ['remote']
  undocumented: convert.git.similarity (int) [50]
  undocumented: convert.hg.clonebranches (bool)
  undocumented: convert.hg.ignoreerrors (bool)
  undocumented: convert.hg.revs (str)
  undocumented: convert.hg.saverev (bool)
  undocumented: convert.hg.sourcename (str)
  undocumented: convert.hg.startrev (str)
  undocumented: convert.hg.tagsbranch (str) ['default']
  undocumented: convert.hg.usebranchnames (bool) [True]
  undocumented: convert.localtimezone (bool)
  undocumented: convert.p4.startrev (str)
  undocumented: convert.skiptags (bool)
  undocumented: convert.svn.debugsvnlog (bool) [True]
  undocumented: convert.svn.startrev (str)
