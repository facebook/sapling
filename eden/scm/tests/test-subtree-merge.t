  $ setconfig diff.git=True
  $ setconfig subtree.cheap-copy=False
  $ setconfig subtree.allow-any-source-commit=True

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
  abort: path 'not-exists' does not exist in commit 255379dc5cbd
  [255]
  $ hg subtree merge --from-path not-exists --to-path bar
  abort: path 'not-exists' does not exist in commit 255379dc5cbd
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

test subtree merge from copy dest -> copy source with conflicts
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
  $ echo "source" >> foo/x && hg ci -m "update foo/x"
  $ echo "dest" >> foo2/x && hg ci -m "update foo2/x"
  $ hg subtree merge --from-path foo2 --to-path foo -t :merge3
  merge base: 9998a5c40732
  merging foo/x and foo2/x to foo/x
  warning: 1 conflicts while merging foo/x! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg st
  M foo/x
  ? foo/x.orig
  $ cat foo/x
  aaa
  <<<<<<< working copy: 33b9c9564908 - test: update foo2/x
  source
  ||||||| base
  =======
  dest
  >>>>>>> merge rev:    33b9c9564908 - test: update foo2/x

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
  merge base: a1e3d459ad62
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
  merge base: a1e3d459ad62
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
  merge base: a1e3d459ad62
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
  merge base: a1e3d459ad62
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
  merge base: eeb423c321b3
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

test subtree merge source commit validation
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
  $ setconfig subtree.allow-any-source-commit=False
  $ hg subtree merge --from-path foo --to-path foo2
  subtree merge from a non-public commit is not recommended. However, you can
  still proceed and use subtree copy and merge for common cases.
  (hint: see 'hg help subtree' for the impacts on subtree merge and log)
  Continue with subtree merge (y/n)?  n
  abort: subtree merge from a non-public commit is not allowed
  [255]

  $ setconfig ui.interactive=True
  $ hg subtree merge --from-path foo --to-path foo2<<EOF
  > y
  > EOF
  subtree merge from a non-public commit is not recommended. However, you can
  still proceed and use subtree copy and merge for common cases.
  (hint: see 'hg help subtree' for the impacts on subtree merge and log)
  Continue with subtree merge (y/n)?  y
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
  $ hg ci -m "merge foo to foo2"
  $ hg show
  commit:      a61481db255e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo2/x
  description:
  merge foo to foo2
  
  Subtree merge from a1e3d459ad62ee74bdfa703d95cd4f63f21fcd3d
  - Merged path foo to foo2
  
  
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,1 +1,2 @@
   aaa
  +source
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default'}
