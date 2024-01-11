#debugruntest-compatible

#testcases copytrace no-copytrace

#if copytrace
  $ setconfig experimental.copytrace=on
#else
  $ setconfig experimental.copytrace=off
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
