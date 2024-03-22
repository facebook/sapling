#debugruntest-compatible

#testcases copytrace no-copytrace

#if copytrace
  $ setconfig copytrace.dagcopytrace=True
#else
  $ setconfig copytrace.dagcopytrace=False
#endif

  $ configure modernclient
  $ newclientrepo
  $ drawdag <<EOS
  > C
  > |
  > B  # B/bar = foo (renamed from foo)
  > |
  > A  # A/foo = foo
  > EOS

  $ hg go -q $C
  $ hg backout -q $B
  $ hg status --change . --copies foo
  A foo
    bar
