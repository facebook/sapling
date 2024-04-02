#debugruntest-compatible

#require no-eden

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
  $ hg backout $B
  merging bar and foo to bar
  0 files updated, 1 files merged, 1 files removed, 0 files unresolved
  changeset be9f9340610a backs out changeset 786106f81394
  $ hg st --change . 
  M bar
  R B
  $ hg diff -r .^ -r . bar
  diff -r 0e278d5079cc -r be9f9340610a bar
  --- a/bar	Thu Jan 01 00:00:00 1970 +0000
  +++ b/bar	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,1 @@
   foo
  -bar
