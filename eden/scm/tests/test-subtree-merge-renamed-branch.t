  $ setconfig diff.git=True
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1
  $ setconfig drawdag.defaultfiles=false

test source directory is renamed
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = y1\ny2\ny3\ny4\ny5\n
  > |
  > A   # A/foo/x = x1\nx2\nx3\nx4\nx5\n
  > EOS
  $ hg go -q $B

  $ hg subtree copy --from-path foo --to-path bar -m "subtree copy foo -> bar"
  copying foo to bar
  $ echo "x1_foo\nx2\nx3\nx4\nx5" > foo/x
  $ hg ci -m "x1_foo"
  $ echo "x1\nx2\nx3_bar\nx4\nx5" > bar/x
  $ hg ci -m "x3_bar"
  $ hg subtree copy --from-path foo --to-path foo1 -m "subtree copy foo -> foo1"
  copying foo to foo1
  $ echo "x1_foo\nx2\nx3\nx4\nx5_foo1" > foo1/x
  $ hg ci -m "x5_foo1"
  $ showgraph 
  @  7c0ced009d62 x5_foo1
  │
  o  17b09a7b6782 subtree copy foo -> foo1
  │
  o  290b1475463b x3_bar
  │
  o  9e4bc5817116 x1_foo
  │
  o  52cd91b6716a subtree copy foo -> bar
  │
  o  9032e003042c B
  │
  o  3203ffa6f201 A
merge the changes of foo and foo1 to bar
  $ hg log -r "subtreemergebase("foo1", "bar")" -T '{node|short}\n'
  9032e003042c
  $ hg subtree merge --from-path foo1 --to-path bar
  searching for merge base ...
  found the last subtree copy commit 52cd91b6716a
  merge base: 9032e003042c
  merging bar/x and foo1/x to bar/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,5 +1,5 @@
  -x1
  +x1_foo
   x2
   x3_bar
   x4
  -x5
  +x5_foo1
  $ hg go -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
merge the change of bar to foo1
  $ hg log -r "subtreemergebase("bar", "foo1")" -T '{node|short}\n'
  9032e003042c
  $ hg subtree merge --from-path bar --to-path foo1
  searching for merge base ...
  found the last subtree copy commit 52cd91b6716a
  merge base: 9032e003042c
  merging foo1/x and bar/x to foo1/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo1/x b/foo1/x
  --- a/foo1/x
  +++ b/foo1/x
  @@ -1,5 +1,5 @@
   x1_foo
   x2
  -x3
  +x3_bar
   x4
   x5_foo1

test both source and dest directories are renamed
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = y1\ny2\ny3\ny4\ny5\n
  > |
  > A   # A/foo/x = x1\nx2\nx3\nx4\nx5\nx6\nx7\n
  > EOS
  $ hg go -q $B

  $ hg subtree copy --from-path foo --to-path bar -m "subtree copy foo -> bar"
  copying foo to bar
  $ echo "x1_foo\nx2\nx3\nx4\nx5\nx6\nx7" > foo/x
  $ hg ci -m "x1_foo"
  $ echo "x1\nx2\nx3_bar\nx4\nx5\nx6\nx7" > bar/x
  $ hg ci -m "x3_bar"
  $ hg subtree copy --from-path foo --to-path foo1 -m "subtree copy foo -> foo1"
  copying foo to foo1
  $ echo "x1_foo\nx2\nx3\nx4\nx5_foo1\nx6\nx7" > foo1/x
  $ hg ci -m "x5_foo1"
  $ hg subtree copy --from-path bar --to-path bar1 -m "subtree copy bar -> bar1"
  copying bar to bar1
  $ echo "x1\nx2\nx3_bar\nx4\nx5\nx6\nx7_bar1" > bar1/x
  $ hg ci -m "x4_bar1"
  $ showgraph 
  @  836d4986b5c5 x4_bar1
  │
  o  9899b7156d0d subtree copy bar -> bar1
  │
  o  11377da2bd86 x5_foo1
  │
  o  a9a3a4d85390 subtree copy foo -> foo1
  │
  o  e41dc8174e1b x3_bar
  │
  o  afbfa0d624d4 x1_foo
  │
  o  e6969568e795 subtree copy foo -> bar
  │
  o  22a57b56f0ee B
  │
  o  0a81f6dee7d1 A
merge the changes of foo and foo1 to bar1
  $ hg log -r "subtreemergebase("foo1", "bar1")" -T '{node|short}\n'
  22a57b56f0ee
  $ hg subtree merge --from-path foo1 --to-path bar1
  searching for merge base ...
  found the last subtree copy commit e6969568e795
  merge base: 22a57b56f0ee
  merging bar1/x and foo1/x to bar1/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/bar1/x b/bar1/x
  --- a/bar1/x
  +++ b/bar1/x
  @@ -1,7 +1,7 @@
  -x1
  +x1_foo
   x2
   x3_bar
   x4
  -x5
  +x5_foo1
   x6
   x7_bar1
  $ hg go -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
merge the change of bar1 and bar to foo1
  $ hg log -r "subtreemergebase("bar1", "foo1")" -T '{node|short}\n'
  22a57b56f0ee
  $ hg subtree merge --from-path bar1 --to-path foo1
  searching for merge base ...
  found the last subtree copy commit e6969568e795
  merge base: 22a57b56f0ee
  merging foo1/x and bar1/x to foo1/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/foo1/x b/foo1/x
  --- a/foo1/x
  +++ b/foo1/x
  @@ -1,7 +1,7 @@
   x1_foo
   x2
  -x3
  +x3_bar
   x4
   x5_foo1
   x6
  -x7
  +x7_bar1
