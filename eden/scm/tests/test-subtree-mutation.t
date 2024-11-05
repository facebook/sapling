  $ setconfig diff.git=True
  $ setconfig subtree.copy-reuse-tree=False
  $ setconfig subtree.allow-any-source-commit=True

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
  @  0a2a689cffc5 subtree copy foo to foo2
  │
  │  Subtree copy from b4cb27eee4e2633aae0d62de87523007d1b5bfdd
  │  - Copied path foo to foo2
  o  c4fbbcdf676b B
  │
  o  b4cb27eee4e2 A
  $ echo 4 >> foo/x
  $ hg amend
  $ hg log -G -T '{node|short} {desc}'
  @  7999ec31e586 subtree copy foo to foo2
  │
  │  Subtree copy from b4cb27eee4e2633aae0d62de87523007d1b5bfdd
  │  - Copied path foo to foo2
  o  c4fbbcdf676b B
  │
  o  b4cb27eee4e2 A
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'amend_source': '0a2a689cffc54d46be66eb9ea4132900c016c1d6', 'test_subtree_copy': '{"branches":[{"from_commit":"b4cb27eee4e2633aae0d62de87523007d1b5bfdd","from_path":"foo","to_path":"foo2"}],"v":1}'}

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
  @  0a2a689cffc5 subtree copy foo to foo2
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
  @  333f270b03aa subtree copy foo to foo2
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
  {'branch': 'default', 'test_subtree_copy': '{"branches":[{"from_commit":"b4cb27eee4e2633aae0d62de87523007d1b5bfdd","from_path":"foo","to_path":"foo2"}],"v":1}'}

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
  @  5c9b51a20940 subtree copy foo to foo2
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
tofix: should merge the subtree metadata
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'test_subtree_copy': '{"branches":[{"from_commit":"b4cb27eee4e2633aae0d62de87523007d1b5bfdd","from_path":"foo","to_path":"foo2"}],"v":1}'}
