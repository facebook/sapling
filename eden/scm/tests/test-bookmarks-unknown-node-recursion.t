A bookmark that points to a node missing from the changelog must not crash
commands. With narrow-heads (the default) and a cold phase cache, loading the
bookmarks used to recurse forever (bmstore -> repo.invalidate ->
invalidatechangelog -> construct phasecache -> heads -> bookmarks -> ...) and
the command died with RecursionError (on Windows, with a native stack
overflow). Now the bad bookmark is skipped with a warning.

  $ newclientrepo
  $ drawdag <<'EOS'
  > A
  > EOS
  $ hg up -q $A

Write a bookmark whose node the changelog does not have (the same state as a
bookmarks file written before the changelog, or after the commit was stripped):

  $ hg debugshell -c 'ml = repo.metalog(); ml["bookmarks"] = b"1111111111111111111111111111111111111111 foo\n"; ml.commit("bookmark to unknown node")'

Every command that loads bookmarks in a fresh process warns and continues:

  $ hg bookmarks
  unknown reference in .hg/bookmarks: foo 1111111111111111111111111111111111111111
  no bookmarks set

  $ hg log -r . -T '{desc} {bookmarks}\n'
  unknown reference in .hg/bookmarks: foo 1111111111111111111111111111111111111111
  A 

  $ hg log -r 'bookmark()' -T '{desc}\n'
  unknown reference in .hg/bookmarks: foo 1111111111111111111111111111111111111111

A command that does not load bookmarks still works:

  $ hg status
