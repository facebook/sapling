  $ setconfig diff.git=True
  $ setconfig subtree.cheap-copy=False

setup backing repo

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS

test subtree merge path validation
  $ hg go -q $B
  $ hg subtree cp -r $A --from-path foo --to-path bar -m "subtree copy foo -> bar"
  copying foo to bar
  $ hg subtree merge --from-path foo --to-path not-exists
  abort: path 'not-exists' does not exist in commit a6c15b42e6a2
  [255]
  $ hg subtree merge --from-path not-exists --to-path bar
  abort: path 'not-exists' does not exist in commit a6c15b42e6a2
  [255]
  $ hg subtree merge --from-path foo/bar --to-path foo
  abort: overlapping --from-path 'foo/bar' and --to-path 'foo'
  [255]
  $ hg subtree merge --from-path foo --to-path foo/bar
  abort: overlapping --from-path 'foo' and --to-path 'foo/bar'
  [255]

test subtree merge from copy source -> copy dest
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy --from-path foo --to-path foo2
  copying foo to foo2
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg subtree merge --from-path foo --to-path foo2
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M foo2/x
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,1 +1,2 @@
   aaa
  +source

test subtree merge from copy dest -> copy source
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy --from-path foo --to-path foo2
  copying foo to foo2
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg subtree merge --from-path foo2 --to-path foo
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M foo/y
  $ hg diff
  diff --git a/foo/y b/foo/y
  --- a/foo/y
  +++ b/foo/y
  @@ -1,1 +1,2 @@
   bbb
  +dest


test multiple subtree merge from source -> dest
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy --from-path foo --to-path foo2
  copying foo to foo2
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg subtree merge --from-path foo --to-path foo2
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "merge foo to foo2"
  $ echo "source2" >> foo/x && hg ci -m "update foo again"
  $ hg subtree merge --from-path foo --to-path foo2
  merge base: 3a47f1b511d0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,2 +1,3 @@
   aaa
   source
  +source2

test multiple subtree merge from dest -> source

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy --from-path foo --to-path foo2
  copying foo to foo2
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg subtree merge --from-path foo2 --to-path foo
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "merge foo2 to foo"
  $ echo "dest2" >> foo2/y && hg ci -m "update foo2 again"
  $ hg subtree merge --from-path foo2 --to-path foo
  merge base: 3a47f1b511d0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo/y b/foo/y
  --- a/foo/y
  +++ b/foo/y
  @@ -1,2 +1,3 @@
   bbb
   dest
  +dest2

test multiple subtree merge from source -> dest, then dest -> source
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy --from-path foo --to-path foo2
  copying foo to foo2
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg subtree merge --from-path foo --to-path foo2
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "merge foo to foo2"
  $ echo "dest2" >> foo2/y && hg ci -m "update foo2 again"
  $ hg subtree merge --from-path foo2 --to-path foo
  merge base: 3a47f1b511d0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo/y b/foo/y
  --- a/foo/y
  +++ b/foo/y
  @@ -1,1 +1,3 @@
   bbb
  +dest
  +dest2

test multiple subtree merge from dest -> source, then source -> dest

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy --from-path foo --to-path foo2
  copying foo to foo2
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg subtree merge --from-path foo2 --to-path foo
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "merge foo2 to foo"
  $ echo "source2" >> foo/x && hg ci -m "update foo again"
  $ hg subtree merge --from-path foo --to-path foo2
  merge base: 3a47f1b511d0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,1 +1,3 @@
   aaa
  +source
  +source2
  $ hg ci -m "merge foo to foo2"
to fix: show a better message when there is no changes for subtree merge
  $ hg subtree merge --from-path foo --to-path foo2
  merge base: 10b55f445a8e
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st

test subtree merge from the same directory from a different branch

  $ newclientrepo
  $ drawdag <<'EOS'
  >   D  # D/foo/y = 111\n
  >   |
  > B C  # B/foo/x = 1a\n2\n3\n
  > |/   # C/foo/x = 1\n2\n3a\n
  > A    # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ hg log -G -T '{node|short} {desc}'
  o  8c8c93854742 D
  │
  o  b1b40873e5ea C
  │
  │ @  c4fbbcdf676b B
  ├─╯
  o  b4cb27eee4e2 A
  $ hg subtree merge -r $C --from-path foo --to-path foo
  merge base: b4cb27eee4e2
  merging foo/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo/x b/foo/x
  --- a/foo/x
  +++ b/foo/x
  @@ -1,3 +1,3 @@
   1a
   2
  -3
  +3a
  $ hg ci -m "merge from foo to foo"

  $ hg go -q $A
  $ hg subtree merge -r $D --from-path foo --to-path foo
  merge base: b4cb27eee4e2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo/x b/foo/x
  --- a/foo/x
  +++ b/foo/x
  @@ -1,3 +1,3 @@
   1
   2
  -3
  +3a
  diff --git a/foo/y b/foo/y
  new file mode 100644
  --- /dev/null
  +++ b/foo/y
  @@ -0,0 +1,1 @@
  +111
  $ hg ci -m "merge foo from a descendant"
