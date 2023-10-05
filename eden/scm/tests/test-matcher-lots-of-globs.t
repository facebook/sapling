#debugruntest-compatible

  $ configure modernclient

Don't crash with lots of glob rules.

  $ newclientrepo
  $ touch foo_0 foo_9999 foo_10000
  >>> open("pats", "w").write("".join(f"glob:**/foo_{i}\n" for i in range(10_000))) and None
  $ hg status listfile:pats
  ? foo_0
  ? foo_9999

Try with longer rules as well:
  >>> open("pats", "w").write("".join("glob:**/%s\n" % ("a"*512) for i in range(10_000))) and None
  $ hg status listfile:pats
