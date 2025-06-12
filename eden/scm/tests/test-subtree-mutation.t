  $ setconfig diff.git=True
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1

test amend subtree copy commit and keep the subtree copy metadata
  $ newclientrepo
  $ drawdag <<'EOS'
  > B  # B/foo/x = 1a\n2\n3\n
  > |
  > A  # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path foo2 -m "subtree copy foo to foo2"
  copying foo to foo2
  $ hg log -G -T '{node|short} {desc}'
  @  d575b719fc35 subtree copy foo to foo2
  │
  │  Subtree copy from b4cb27eee4e2633aae0d62de87523007d1b5bfdd
  │  - Copied path foo to foo2
  o  c4fbbcdf676b B
  │
  o  b4cb27eee4e2 A
  $ echo 4 >> foo/x
  $ hg amend
  $ hg log -G -T '{node|short} {desc}'
  @  0cad5f90151f subtree copy foo to foo2
  │
  │  Subtree copy from b4cb27eee4e2633aae0d62de87523007d1b5bfdd
  │  - Copied path foo to foo2
  o  c4fbbcdf676b B
  │
  o  b4cb27eee4e2 A
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'amend_source': 'd575b719fc35d1f76d70ce1a76e37baa7274e283', 'test_subtree': '[{"deepcopies":[{"from_commit":"b4cb27eee4e2633aae0d62de87523007d1b5bfdd","from_path":"foo","to_path":"foo2"}],"v":1}]'}

test fold subtree copy commit and keep the subtree copy metadata
  $ newclientrepo
  $ drawdag <<'EOS'
  > B  # B/foo/x = 1a\n2\n3\n
  > |
  > A  # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path foo2 -m "subtree copy foo to foo2"
  copying foo to foo2
  $ hg log -G -T '{node|short} {desc}'
  @  d575b719fc35 subtree copy foo to foo2
  │
  │  Subtree copy from b4cb27eee4e2633aae0d62de87523007d1b5bfdd
  │  - Copied path foo to foo2
  o  c4fbbcdf676b B
  │
  o  b4cb27eee4e2 A
  $ echo 4 >> foo/x
  $ hg ci -m 'update foo/x'
  $ hg fold --from .^
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T '{node|short} {desc}'
  @  9a2f428954c2 subtree copy foo to foo2
  │
  │  Subtree copy from b4cb27eee4e2633aae0d62de87523007d1b5bfdd
  │  - Copied path foo to foo2
  │
  │
  │  update foo/x
  o  c4fbbcdf676b B
  │
  o  b4cb27eee4e2 A
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'test_subtree': '[{"deepcopies":[{"from_commit":"b4cb27eee4e2633aae0d62de87523007d1b5bfdd","from_path":"foo","to_path":"foo2"}],"v":1}]'}

test fold two subtree copy commits and merge the subtree copy metadata
  $ newclientrepo
  $ drawdag <<'EOS'
  > B  # B/foo/x = 1a\n2\n3\n
  > |
  > A  # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path foo2 -m "subtree copy foo to foo2"
  copying foo to foo2
  $ hg subtree copy -r $A --from-path foo --to-path foo3 -m "subtree copy foo to foo3"
  copying foo to foo3
  $ hg fold --from .^
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T '{node|short} {desc}'
  @  b65a9d8c0c21 subtree copy foo to foo2
  │
  │  Subtree copy from b4cb27eee4e2633aae0d62de87523007d1b5bfdd
  │  - Copied path foo to foo2
  │
  │
  │  subtree copy foo to foo3
  │
  │  Subtree copy from b4cb27eee4e2633aae0d62de87523007d1b5bfdd
  │  - Copied path foo to foo3
  o  c4fbbcdf676b B
  │
  o  b4cb27eee4e2 A
should merge the subtree metadata
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  test_subtree=[{"deepcopies":[{"from_commit":"b4cb27eee4e2633aae0d62de87523007d1b5bfdd","from_path":"foo","to_path":"foo2"},{"from_commit":"b4cb27eee4e2633aae0d62de87523007d1b5bfdd","from_path":"foo","to_path":"foo3"}],"v":1}]

test fold two subtree copy commits that have path overlapping
  $ newclientrepo
  $ drawdag <<'EOS'
  > B  # B/foo/x = 1a\n2\n3\n
  > |
  > A  # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path bar -m "subtree copy foo to bar"
  copying foo to bar
  $ hg subtree copy -r $A --from-path foo --to-path bar/foo2 -m "subtree copy foo to bar/foo2"
  copying foo to bar/foo2
  $ hg fold --from .^
  abort: cannot combine commits with overlapping subtree paths
  (overlapping --to-path entries)
  [255]

test fold subtree copy and subtree merge commits
  $ newclientrepo
  $ drawdag <<'EOS'
  > B  # B/foo/x = 1a\n2\n3\n
  > |
  > A  # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path bar -m "subtree copy foo to bar"
  copying foo to bar
  $ hg go -q $A
  $ echo 4 >> foo/x
  $ hg ci -m 'update on master side'
  $ hg log -G -T '{node|short} {desc}'
  @  fe8ce627cbe8 update on master side
  │
  │ o  ee6785824a72 subtree copy foo to bar
  │ │
  │ │  Subtree copy from b4cb27eee4e2633aae0d62de87523007d1b5bfdd
  │ │  - Copied path foo to bar
  │ o  c4fbbcdf676b B
  ├─╯
  o  b4cb27eee4e2 A
  $ hg go -q ee6785824a72
  $ hg subtree merge -r fe8ce627cbe8 --from-path foo --to-path bar
  searching for merge base ...
  found the last subtree copy commit ee6785824a72
  merge base: b4cb27eee4e2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,4 @@
   1
   2
   3
  +4
  $ hg ci -m 'merge from foo to bar'
  $ hg fold --from .^
  abort: cannot combine different subtree operations: copy, merge or import
  [255]

test split subtree copy commit
  $ newclientrepo
  $ drawdag <<'EOS'
  > B  # B/foo/x = 1a\n2\n3\n
  > |
  > A  # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path bar -m "subtree copy foo to bar"
  copying foo to bar
  $ hg split
  abort: cannot split subtree copy/merge commits
  [255]

test graft subtree copy and merge commits
  $ newclientrepo
  $ drawdag <<'EOS'
  > B  # B/foo/x = 1a\n2\n3\n
  > |
  > A  # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ hg subtree copy -r $A --from-path foo --to-path bar -m "subtree copy foo to bar"
  copying foo to bar
  $ hg subtree merge -r $B --from-path foo --to-path bar -q
  $ hg ci -m "subtree merge foo to bar"
  $ hg log -G -T '{node|short} {desc|firstline}'
  @  5d2f9c7b4852 subtree merge foo to bar
  │
  o  ee6785824a72 subtree copy foo to bar
  │
  o  c4fbbcdf676b B
  │
  o  b4cb27eee4e2 A
  $ hg go -q $B
  $ hg graft 5d2f9c7b4852 ee6785824a72
  skipping ungraftable subtree (copy, merge, import) revision 5d2f9c7b4852
  skipping ungraftable subtree (copy, merge, import) revision ee6785824a72
  abort: empty revision set was specified
  [255]

enables grafting of subtree commits without retaining subtree metadata
  $ hg graft ee6785824a72 --config subtree.allow-graft-subtree-commit=True
  grafting ee6785824a72 "subtree copy foo to bar"
  $ hg log -G -T '{node|short} {desc|firstline}'
  @  aa57c2d74172 subtree copy foo to bar
  │
  │ o  5d2f9c7b4852 subtree merge foo to bar
  │ │
  │ o  ee6785824a72 subtree copy foo to bar
  ├─╯
  o  c4fbbcdf676b B
  │
  o  b4cb27eee4e2 A
  $ hg subtree inspect -r .
  no subtree metadata found for commit aa57c2d74172
