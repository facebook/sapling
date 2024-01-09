#debugruntest-compatible

  $ configure modernclient
  $ enable sparse

Don't crash with lots of rules

  $ newclientrepo
  >>> open(".hg/sparse", "w").write("".join(f"path:foo_{i}\n" for i in range(10_000))) and None
  $ hg status
