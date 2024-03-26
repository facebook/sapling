#debugruntest-compatible

  $ enable copytrace
  $ setconfig copytrace.dagcopytrace=True

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

test back out a commit before rename

  $ newclientrepo
  $ drawdag <<EOS
  > C  # C/bar = foo\nbar\n (renamed from foo)
  > |
  > B  # B/foo = foo\nbar\n
  > |
  > A  # A/foo = foo\n
  > EOS

  $ hg go -q $C

backout should be succeeded (tofix)

  $ hg backout $B
  other changed foo which local is missing
  hint: if this is due to a renamed file, you can manually input the renamed path
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  0 files updated, 0 files merged, 1 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]
