  $ setconfig diff.git=True
  $ setconfig subtree.cheap-copy=False
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1

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

test subtree merge max file count
  $ newclientrepo
  $ drawdag <<'EOS'
  >   D  # D/foo/y = 111\n
  >   |
  > B C  # B/foo/x = 1a\n2\n3\n
  > |/   # C/foo/x = 1\n2\n3a\n
  > A    # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ hg subtree merge --from-path foo --to-path foo --rev $D --config subtree.max-file-count=1
  abort: subtree path 'foo' includes too many files: 2 (max: 1)
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
  $ hg log -r "subtreemergebase("foo", "foo2")" -T '{node|short}\n'
  9998a5c40732
  $ hg subtree merge --from-path foo --to-path foo2
  searching for merge base ...
  found the last subtree copy commit 39067344b0b6
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
  $ hg log -r "subtreemergebase("foo2", "foo")" -T '{node|short}\n'
  9998a5c40732
  $ hg subtree merge --from-path foo2 --to-path foo
  searching for merge base ...
  found the last subtree copy commit 39067344b0b6
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

test subtree merge from normal copy source -> copy dest
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg cp -q foo foo2 && hg ci -m 'cp foo -> foo2'
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg log -r "subtreemergebase("foo", "foo2")" -T '{node|short}\n'
  9998a5c40732
  $ hg subtree merge --from-path foo --to-path foo2
  searching for merge base ...
  merge base: 9998a5c40732
  merging foo2/x and foo/x to foo2/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
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

test subtree merge from noraml copy dest -> copy source
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg cp -q foo foo2 && hg ci -m 'cp foo -> foo2'
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg log -G -T '{node|short} {desc}\n'
  @  6267dbdc54d7 update foo2
  │
  o  12e6fb3107e7 update foo
  │
  o  e5df3da1ae9c cp foo -> foo2
  │
  o  9998a5c40732 B
  │
  o  d908813f0f7c A
  $ hg log -r "subtreemergebase("foo2", "foo")" -T '{node|short}\n'
  9998a5c40732
  $ hg subtree merge --from-path foo2 --to-path foo 
  searching for merge base ...
  merge base: 9998a5c40732
  merging foo/x and foo2/x to foo/x
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M foo/x
  M foo/y
  $ hg diff
  diff --git a/foo/y b/foo/y
  --- a/foo/y
  +++ b/foo/y
  @@ -1,1 +1,2 @@
   bbb
  +dest

test subtree merge without copy info: foo2 -> foo
  $ newclientrepo
  $ drawdag <<'EOS'
  > C   # C/foo2/y = bbb\n
  > |   # C/foo2/x = aaa\nbbb\n
  > |   # C/foo/y = bbb\nccc\n
  > |
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $C
  $ hg log -G -T '{node|short} {desc}\n'
  @  4c02b9463eef C
  │
  o  9998a5c40732 B
  │
  o  d908813f0f7c A
  $ hg log -r "subtreemergebase("foo2", "foo")" -T '{node|short}\n'
  9998a5c40732
  $ hg subtree merge --from-path foo2 --to-path foo
  searching for merge base ...
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M foo/x
  $ hg diff
  diff --git a/foo/x b/foo/x
  --- a/foo/x
  +++ b/foo/x
  @@ -1,1 +1,2 @@
   aaa
  +bbb

test subtree merge without copy info: foo -> foo2
  $ newclientrepo
  $ drawdag <<'EOS'
  > C   # C/foo2/y = bbb\n
  > |   # C/foo2/x = aaa\nbbb\n
  > |   # C/foo/y = bbb\nccc\n
  > |
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $C
  $ hg log -G -T '{node|short} {desc}\n'
  @  4c02b9463eef C
  │
  o  9998a5c40732 B
  │
  o  d908813f0f7c A
  $ hg log -r "subtreemergebase("foo", "foo2")" -T '{node|short}\n'
  9998a5c40732
  $ hg subtree merge --from-path foo --to-path foo2
  searching for merge base ...
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M foo2/y
  $ hg diff
  diff --git a/foo2/y b/foo2/y
  --- a/foo2/y
  +++ b/foo2/y
  @@ -1,1 +1,2 @@
   bbb
  +ccc

test subtree merge from copy dest -> copy source, with new file in copy dest
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
  $ echo 1 >> foo2/new
  $ hg ci -Aqm "add foo2/new"
  $ hg log -r "subtreemergebase("foo2", "foo")" -T '{node|short}\n'
  9998a5c40732
  $ hg subtree merge --from-path foo2 --to-path foo
  searching for merge base ...
  found the last subtree copy commit 39067344b0b6
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M foo/new
  $ hg diff
  diff --git a/foo/new b/foo/new
  new file mode 100644
  --- /dev/null
  +++ b/foo/new
  @@ -0,0 +1,1 @@
  +1

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
  $ hg log -r "subtreemergebase("foo2", "foo")" -T '{node|short}\n'
  9998a5c40732
  $ hg subtree merge --from-path foo2 --to-path foo -t :merge3
  searching for merge base ...
  found the last subtree copy commit 39067344b0b6
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
  searching for merge base ...
  found the last subtree copy commit 39067344b0b6
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "merge foo to foo2"
  $ echo "source2" >> foo/x && hg ci -m "update foo again"
  $ hg subtree merge --from-path foo --to-path foo2
  searching for merge base ...
  found the last subtree merge commit a26d75b3506f
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
  searching for merge base ...
  found the last subtree copy commit 39067344b0b6
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "merge foo2 to foo"
  $ echo "dest2" >> foo2/y && hg ci -m "update foo2 again"
  $ hg subtree merge --from-path foo2 --to-path foo
  searching for merge base ...
  found the last subtree merge commit cd2a696dab0f
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
  searching for merge base ...
  found the last subtree copy commit 39067344b0b6
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,1 +1,2 @@
   aaa
  +source
  $ hg ci -m "merge foo to foo2"
  $ echo "dest2" >> foo2/x
  $ echo "dest2" >> foo2/y && hg ci -m "update foo2 again"
  $ hg subtree merge --from-path foo2 --to-path foo
  searching for merge base ...
  found the last subtree merge commit a26d75b3506f
  merge base: a1e3d459ad62
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo/x b/foo/x
  --- a/foo/x
  +++ b/foo/x
  @@ -1,2 +1,3 @@
   aaa
   source
  +dest2
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
  searching for merge base ...
  found the last subtree copy commit 39067344b0b6
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "merge foo2 to foo"
  $ echo "source2" >> foo/x && hg ci -m "update foo again"
  $ hg subtree merge --from-path foo --to-path foo2
  searching for merge base ...
  found the last subtree merge commit cd2a696dab0f
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
  $ hg log -r . -T '{subtree_merges}\n'
  [{"version": 1, "from_commit": "eeb423c321b3fae8bffd501cecd7db6d8fa9b6da", "from_path": "foo", "to_path": "foo2"}]
to fix: show a better message when there is no changes for subtree merge
  $ hg subtree merge --from-path foo --to-path foo2
  searching for merge base ...
  found the last subtree merge commit 62391083dc84
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
  searching for merge base ...
  found the last subtree copy commit 39067344b0b6
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

test subtree merge with different merge tools

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = 1foo\n2\n3\n4\n5foo\n
  > |   # B/foo/y = yfoo\n
  > |
  > A   # A/foo/x = 1\n2\n3\n4\n5\n
  >     # A/foo/y = y\n
  >     # A/foo/z = z\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path foo2 -m "subtree copy foo -> foo2"
  copying foo to foo2
  $ cat > foo2/x <<EOF
  > 1
  > 2
  > 3foo2
  > 4
  > 5foo2
  > EOF
  $ echo "zfoo2" > foo2/z
  $ hg ci -m "update foo2"
  $ hg log -G -T '{node|short} {desc}\n'
  @  37f56915baff update foo2
  │
  o  dfb7e4c6a0af subtree copy foo -> foo2
  │
  │  Subtree copy from d1c0dec1161c673fb29a7658c93fd0000c9793ff
  │  - Copied path foo to foo2
  o  1fe9e555c542 B
  │
  o  d1c0dec1161c A

  $ hg subtree merge --from-path foo --to-path foo2 -t :other
  searching for merge base ...
  found the last subtree copy commit dfb7e4c6a0af
  merge base: d1c0dec1161c
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,5 +1,5 @@
  -1
  +1foo
   2
  -3foo2
  +3
   4
  -5foo2
  +5foo
  diff --git a/foo2/y b/foo2/y
  --- a/foo2/y
  +++ b/foo2/y
  @@ -1,1 +1,1 @@
  -y
  +yfoo
  $ hg go -C . -q && hg clean

  $ hg subtree merge --from-path foo --to-path foo2 -t :merge-other
  searching for merge base ...
  found the last subtree copy commit dfb7e4c6a0af
  merge base: d1c0dec1161c
  merging foo2/x and foo/x to foo2/x
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,5 +1,5 @@
  -1
  +1foo
   2
   3foo2
   4
  -5foo2
  +5foo
  diff --git a/foo2/y b/foo2/y
  --- a/foo2/y
  +++ b/foo2/y
  @@ -1,1 +1,1 @@
  -y
  +yfoo
  $ hg go -C . -q && hg clean

  $ hg subtree merge --from-path foo --to-path foo2 -t :local
  searching for merge base ...
  found the last subtree copy commit dfb7e4c6a0af
  merge base: d1c0dec1161c
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo2/y b/foo2/y
  --- a/foo2/y
  +++ b/foo2/y
  @@ -1,1 +1,1 @@
  -y
  +yfoo
  $ hg go -C . -q && hg clean

  $ hg subtree merge --from-path foo --to-path foo2 -t :merge-local
  searching for merge base ...
  found the last subtree copy commit dfb7e4c6a0af
  merge base: d1c0dec1161c
  merging foo2/x and foo/x to foo2/x
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,4 +1,4 @@
  -1
  +1foo
   2
   3foo2
   4
  diff --git a/foo2/y b/foo2/y
  --- a/foo2/y
  +++ b/foo2/y
  @@ -1,1 +1,1 @@
  -y
  +yfoo
  $ hg go -C . -q && hg clean

test deleted/changed conflict

  $ newclientrepo
  $ drawdag <<'EOS'
  > B    # B/foo/x = (removed)
  > |
  > A    # A/foo/x = 1\n2\n3\n
  > |    # A/foo/y = y\n
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path foo2 -m "subtree copy foo -> foo2"
  copying foo to foo2
  $ echo "foo2" >> foo2/x
  $ hg ci -m "update foo2/x"
  $ hg subtree merge --from-path foo2 --to-path foo
  searching for merge base ...
  found the last subtree copy commit 7cc73f2f18dc
  merge base: f7de0a4f3e86
  other [merge rev] changed foo/x which local [working copy] is missing
  hint: if this is due to a renamed file, you can manually input the renamed path
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

test merge base strategy: only search to-path history
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy --from-path foo --to-path foo2 -m "subtree copy foo to foo2"
  copying foo to foo2
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  fff853b992f4 update foo2
  │
  o  ae493dfc8f2a update foo
  │
  o  28f8d78804e0 subtree copy foo to foo2
  │
  o  9998a5c40732 B
  │
  o  d908813f0f7c A
  $ hg subtree merge --from-path foo --to-path foo2 --verbose
  searching for merge base ...
  strategy_pathhisotry stats for 'foo2': * (glob)
  strategy_pathhisotry stats for 'foo': * (glob)
  found the last subtree copy commit 28f8d78804e0
  merge base: 9998a5c40732
  resolving manifests
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg go -C . -q
  $ hg subtree merge --from-path foo --to-path foo2 --merge-base-strategy only-to --verbose
  searching for merge base ...
  strategy_pathhisotry stats for 'foo2': * (glob)
  found the last subtree copy commit 28f8d78804e0
  merge base: 9998a5c40732
  resolving manifests
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)

test merge base strategy: only search from-path history
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy --from-path foo --to-path foo2 -m "subtree copy foo to foo2"
  copying foo to foo2
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  fff853b992f4 update foo2
  │
  o  ae493dfc8f2a update foo
  │
  o  28f8d78804e0 subtree copy foo to foo2
  │
  o  9998a5c40732 B
  │
  o  d908813f0f7c A
  $ hg subtree merge --from-path foo2 --to-path foo --verbose
  searching for merge base ...
  strategy_pathhisotry stats for 'foo': * (glob)
  strategy_pathhisotry stats for 'foo2': * (glob)
  found the last subtree copy commit 28f8d78804e0
  merge base: 9998a5c40732
  resolving manifests
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo/y b/foo/y
  --- a/foo/y
  +++ b/foo/y
  @@ -1,1 +1,2 @@
   bbb
  +dest
  $ hg go -C . -q
  $ hg subtree merge --from-path foo2 --to-path foo --merge-base-strategy only-from --verbose
  searching for merge base ...
  strategy_pathhisotry stats for 'foo2': * (glob)
  found the last subtree copy commit 28f8d78804e0
  merge base: 9998a5c40732
  resolving manifests
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)

test subtree merge from subtree copy commit

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = 1foo\n2\n3\n4\n5foo\n
  > |
  > A   # A/foo/x = 1\n2\n3\n4\n5\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path foo2 -m "subtree copy foo -> foo2"
  copying foo to foo2
  $ cat > foo2/x <<EOF
  > 1
  > 2
  > 3foo2
  > 4
  > 5
  > EOF
  $ hg ci -m "update foo2"
  $ hg subtree merge -r .^ --from-path foo --to-path foo2
  searching for merge base ...
  found the last subtree copy commit 9b7364fcbb0b
  merge base: 0a99ffb8a8f3
  merging foo2/x and foo/x to foo2/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,5 +1,5 @@
  -1
  +1foo
   2
   3foo2
   4
  -5
  +5foo

test subtree merge with subtree copy overwriting a path

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = 1foo\n2\n3\n4\n5foo\n
  > |
  > A   # A/foo/x = 1\n2\n3\n4\n5\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path foo2 -m "subtree copy foo -> foo2"
  copying foo to foo2
  $ hg subtree merge --from-path foo --to-path foo2
  searching for merge base ...
  found the last subtree copy commit 9b7364fcbb0b
  merge base: 0a99ffb8a8f3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "subtree merge foo to foo2"
  $ hg subtree copy -r 9b7364fcbb0b --from-path foo2 --to-path foo2 -m "overwrite foo2 -> foo2" --force
  removing foo2/x
  copying foo2 to foo2
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  e27d8148b043 overwrite foo2 -> foo2
  │
  o  a5697510be4b subtree merge foo to foo2
  │
  o  9b7364fcbb0b subtree copy foo -> foo2
  │
  o  e4d1c0766aec B
  │
  o  0a99ffb8a8f3 A
  $ hg log -r "subtreemergebase("foo", "foo2")" -T '{node|short}\n'
  0a99ffb8a8f3
  $ hg subtree merge --from-path foo --to-path foo2
  searching for merge base ...
  found the last subtree copy commit 9b7364fcbb0b
  merge base: 0a99ffb8a8f3
  merging foo2/x and foo/x to foo2/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,5 +1,5 @@
  -1
  +1foo
   2
   3
   4
  -5
  +5foo
