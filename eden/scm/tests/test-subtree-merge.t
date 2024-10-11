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
  abort: path 'not-exists' does not exist in commit d7a063467d35
  [255]
  $ hg subtree merge --from-path not-exists --to-path bar
  abort: path 'not-exists' does not exist in commit d7a063467d35
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
