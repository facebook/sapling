#require no-windows

A bookmark that points to a node missing from the changelog must not crash
commands. This test pins the CURRENT (incorrect) behavior: with narrow-heads
(the default) and a cold phase cache, loading the bookmarks recurses forever
(bmstore -> repo.invalidate -> invalidatechangelog -> construct phasecache ->
heads -> bookmarks -> ...) and the command dies with RecursionError.

On Windows the same recursion overflows the native stack before Python raises
RecursionError ("thread 'main' has overflowed its stack", exit code
0xC00000FD), so the crash cannot be asserted there. The next commit fixes the
crash and removes the gate, so Windows covers the corrected behavior.

  $ newclientrepo
  $ drawdag <<'EOS'
  > A
  > EOS
  $ hg up -q $A

Write a bookmark whose node the changelog does not have (the same state as a
bookmarks file written before the changelog, or after the commit was stripped):

  $ hg debugshell -c 'ml = repo.metalog(); ml["bookmarks"] = b"1111111111111111111111111111111111111111 foo\n"; ml.commit("bookmark to unknown node")'

Every command that loads bookmarks in a fresh process crashes:

  $ hg bookmarks 2>&1 | grep -E '^(RecursionError|unknown reference|no bookmarks)'
  RecursionError: maximum recursion depth exceeded

  $ hg log -r . -T '{desc} {bookmarks}\n' 2>&1 | grep -E '^(RecursionError|unknown reference|A)'
  RecursionError: maximum recursion depth exceeded

  $ hg log -r 'bookmark()' -T '{desc}\n' 2>&1 | grep -E '^(RecursionError|unknown reference|A)'
  RecursionError: maximum recursion depth exceeded

A command that does not load bookmarks still works:

  $ hg status
