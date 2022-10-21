#chg-compatible
#debugruntest-compatible

  $ configure modernclient
  $ enable sparse
  $ setconfig workingcopy.use-rust=true

Don't crash with lots of rules

  $ newclientrepo
  >>> open(".hg/sparse", "w").write("".join(f"path:foo_{i}\n" for i in range(10_000))) and None
  $ hg status --config status.use-rust=false
  $ hg status --config status.use-rust=true
