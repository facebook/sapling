
#require no-eden

#testcases tree-store tree-manifest

#if tree-manifest
  $ setconfig experimental.pathhistory-treemanifest=true
#endif

  $ setconfig copytrace.dagcopytrace=True
  $ setconfig diff.git=true

  $ newclientrepo
  $ drawdag <<EOS
  > C
  > |
  > B  # B/bar = foo (renamed from foo)
  > |
  > A  # A/foo = foo
  > EOS

  $ sl go -q $C
  $ sl backout -q $B
  $ sl status --change . --copies foo
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

  $ sl go -q $C
  $ sl backout $B
  merging bar and foo to bar
  0 files updated, 1 files merged, 1 files removed, 0 files unresolved
  changeset be9f9340610a backs out changeset 786106f81394
  $ sl st --change . 
  M bar
  R B
  $ sl diff -r .^ -r . bar
  diff --git a/bar b/bar
  --- a/bar
  +++ b/bar
  @@ -1,2 +1,1 @@
   foo
  -bar

Back out a commit copying and modifying a file:

  $ newclientrepo
  $ drawdag <<EOF
  > C
  > |
  > B  # B/B = B (copied from A)
  > |
  > A
  > EOF
  $ sl go -q $C
  $ sl backout -r $B
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  changeset 45998aa158d6 backs out changeset 6910c7fd50e3
  $ sl show
  commit:      45998aa158d6
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A B
  description:
  Back out "B"
  
  Original commit changeset: 6910c7fd50e3
  
  
  diff --git a/B b/B
  deleted file mode 100644
  --- a/B
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -B
  \ No newline at end of file

Back out a commit copying and modifying a file (case 2)

  $ newclientrepo
  $ drawdag <<EOF
  > C  # C/A = AA
  > |
  > B  # B/B = B (copied from A)
  > |
  > A
  > EOF
  $ sl go -q $C
  $ sl backout -r $B
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  changeset c4c12f133dca backs out changeset 6910c7fd50e3
  $ sl show
  commit:      c4c12f133dca
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A B
  description:
  Back out "B"
  
  Original commit changeset: 6910c7fd50e3
  
  
  diff --git a/B b/B
  deleted file mode 100644
  --- a/B
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -B
  \ No newline at end of file
