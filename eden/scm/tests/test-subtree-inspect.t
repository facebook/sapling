  $ setconfig diff.git=True
  $ setconfig subtree.cheap-copy=False
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1

test subtree inspect for subtree metadata
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg subtree inspect -r $B
  no subtree metadata found for commit 9998a5c40732
  $ hg go -q $B
  $ hg subtree copy --from-path foo --to-path foo2
  copying foo to foo2
  $ hg subtree inspect
  {
    "copies": [
      {
        "version": 1,
        "from_commit": "9998a5c40732fc326e6f10a4f14437c7f8e8e7ae",
        "from_path": "foo",
        "to_path": "foo2",
        "type": "deepcopy"
      }
    ]
  }
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
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
  $ hg ci -m "merge foo to foo2"
  $ hg subtree inspect
  {
    "merges": [
      {
        "version": 1,
        "from_commit": "a1e3d459ad62ee74bdfa703d95cd4f63f21fcd3d",
        "from_path": "foo",
        "to_path": "foo2"
      }
    ]
  }
