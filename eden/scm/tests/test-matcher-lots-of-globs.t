#debugruntest-compatible

  $ configure modernclient

Don't crash with lots of glob rules. This is particularly important on
case insensitive filesystems since the globs are converted to a giant
regex by the glob library.

  $ newclientrepo
  $ touch foo_0 foo_9999 foo_10000
  >>> open("pats", "w").write("".join(f"glob:**/foo_{i}\n" for i in range(10_000))) and None
  $ hg status listfile:pats
  ? foo_0
  ? foo_9999
