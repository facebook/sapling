  $ setconfig diff.git=True
  $ setconfig subtree.cheap-copy=False
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1

test subtree inspect for subtree metadata
  $ newclientrepo
  $ drawdag <<'EOS'
  > C   # C/foo/x = aaa\nccc\n
  > |
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS

  $ hg log -G -T '{node|short} {desc}\n'
  o  eed7ada653c1 C
  │
  o  9998a5c40732 B
  │
  o  d908813f0f7c A
  $ hg go -q $C
  $ hg subtree copy -r $B --from-path foo --to-path foo2
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
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  test_subtree=[{"deepcopies":[{"from_commit":"9998a5c40732fc326e6f10a4f14437c7f8e8e7ae","from_path":"foo","to_path":"foo2"}],"v":1}]

enable the new subtree key
  $ setconfig subtree.use-prod-subtree-key=True
  $ hg dbsh -c "print(sapling.utils.subtreeutil.get_subtree_key(ui))"
  subtree

make sure inspect command works for existing metadata
  $ hg subtree inspect -r .
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

make sure fold can combine old and new subtree keys
  $ hg subtree copy -r $A --from-path foo --to-path foo3
  copying foo to foo3
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"deepcopies":[{"from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b","from_path":"foo","to_path":"foo3"}],"v":1}]
  $ hg fold --from .^
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"deepcopies":[{"from_commit":"9998a5c40732fc326e6f10a4f14437c7f8e8e7ae","from_path":"foo","to_path":"foo2"},{"from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b","from_path":"foo","to_path":"foo3"}],"v":1}]

merge should use commit B (9998a5c40732) as the merge base
  $ echo "source" >> foo/x && hg ci -m "update foo"
  $ echo "dest" >> foo2/y && hg ci -m "update foo2"
  $ hg subtree merge --from-path foo --to-path foo2 -t :merge3
  searching for merge base ...
  found the last subtree copy commit 2b794ff58e31
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M foo2/x
  $ hg diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,1 +1,3 @@
   aaa
  +ccc
  +source
  $ hg ci -m "merge foo to foo2"
  $ hg subtree inspect
  {
    "merges": [
      {
        "version": 1,
        "from_commit": "03dfd4b086085a00e29f7e8d55db1880e8bd0190",
        "from_path": "foo",
        "to_path": "foo2"
      }
    ]
  }
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"merges":[{"from_commit":"03dfd4b086085a00e29f7e8d55db1880e8bd0190","from_path":"foo","to_path":"foo2"}],"v":1}]

disable the new subtree key and make sure inspect command works for existing metadata
  $ setconfig subtree.use-prod-subtree-key=False
  $ hg dbsh -c "print(sapling.utils.subtreeutil.get_subtree_key(ui))"
  test_subtree
  $ hg subtree inspect
  {
    "merges": [
      {
        "version": 1,
        "from_commit": "03dfd4b086085a00e29f7e8d55db1880e8bd0190",
        "from_path": "foo",
        "to_path": "foo2"
      }
    ]
  }
