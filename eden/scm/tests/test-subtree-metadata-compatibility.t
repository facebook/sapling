  $ setconfig diff.git=True
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

  $ sl log -G -T '{node|short} {desc}\n'
  o  eed7ada653c1 C
  │
  o  9998a5c40732 B
  │
  o  d908813f0f7c A
  $ sl go -q $C
  $ sl subtree copy -r $B --from-path foo --to-path foo2
  copying foo to foo2
  $ sl subtree inspect
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
  $ sl log -r . -T '{extras % "{extra}\n"}'
  branch=default
  test_subtree=[{"deepcopies":[{"from_commit":"9998a5c40732fc326e6f10a4f14437c7f8e8e7ae","from_path":"foo","to_path":"foo2"}],"v":1}]

enable the new subtree key
  $ setconfig subtree.use-prod-subtree-key=True
  $ sl dbsh -c "print(sapling.utils.subtreeutil.get_subtree_key(ui))"
  subtree

make sure inspect command works for existing metadata
  $ sl subtree inspect -r .
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
  $ sl subtree copy -r $A --from-path foo --to-path foo3
  copying foo to foo3
  $ sl log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"deepcopies":[{"from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b","from_path":"foo","to_path":"foo3"}],"v":1}]
  $ sl fold --from .^
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"deepcopies":[{"from_commit":"9998a5c40732fc326e6f10a4f14437c7f8e8e7ae","from_path":"foo","to_path":"foo2"},{"from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b","from_path":"foo","to_path":"foo3"}],"v":1}]

merge should use commit B (9998a5c40732) as the merge base
  $ echo "source" >> foo/x && sl ci -m "update foo"
  $ echo "dest" >> foo2/y && sl ci -m "update foo2"
  $ sl subtree merge --from-path foo --to-path foo2 -t :merge3
  searching for merge base ...
  found the last subtree copy commit 2b794ff58e31
  merge base: 9998a5c40732
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ sl st
  M foo2/x
  $ sl diff
  diff --git a/foo2/x b/foo2/x
  --- a/foo2/x
  +++ b/foo2/x
  @@ -1,1 +1,3 @@
   aaa
  +ccc
  +source
  $ sl ci -m "merge foo to foo2"
  $ sl subtree inspect
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
  $ sl log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"merges":[{"from_commit":"03dfd4b086085a00e29f7e8d55db1880e8bd0190","from_path":"foo","to_path":"foo2"}],"v":1}]

disable the new subtree key and make sure inspect command works for existing metadata
  $ setconfig subtree.use-prod-subtree-key=False
  $ sl dbsh -c "print(sapling.utils.subtreeutil.get_subtree_key(ui))"
  test_subtree
  $ sl subtree inspect
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

legacy shallow copy metadata ("copies") is ignored
  $ newclientrepo
  $ drawdag <<'EOS'
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ sl go -q $A
  $ mkdir bar
  $ cp foo/x bar/x
  $ sl add -q bar/x
  $ sl dbsh << 'EOS'
  > import json
  > metadata = [{"copies": [{"from_commit": "d908813f0f7c9078810e26aad1e37bdb32013d4b", "from_path": "foo", "to_path": "bar"}], "v": 1}]
  > repo.commit("legacy shallow copy", extra={"test_subtree": json.dumps(metadata, separators=(",", ":"))})
  > EOS
  $ sl log -r . -T '{desc}\n{files}\n'
  legacy shallow copy
  bar/x
  $ sl log -r . -T '{subtree_copies}\n'
  $ sl subtree inspect
  no subtree metadata found for commit bc254fa92aa3
  $ echo bbb >> bar/x
  $ sl amend
  $ sl log -r . -T '{files}\n'
  bar/x
  $ sl subtree copy -r $A --from-path foo --to-path baz -m "deep copy"
  copying foo to baz
  $ sl fold --from .^
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl log -r . -T '{extras % "{extra}\n"}'
  amend_source=bc254fa92aa38bbdec88f9cb33d1d18b8768e91f
  branch=default
  test_subtree=[{"deepcopies":[{"from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b","from_path":"foo","to_path":"baz"}],"v":1}]
