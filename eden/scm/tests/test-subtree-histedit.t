  $ setconfig diff.git=True
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1
  $ enable histedit

test histedit to fold subtree merge commits
  $ newclientrepo
  $ drawdag <<'EOS'
  > D  # D/foo/x = 1a\n2a\n3a\n
  > |
  > C  # C/foo/x = 1a\n2a\n3\n
  > |
  > B  # B/foo/x = 1a\n2\n3\n
  > |
  > A  # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $D
  $ hg subtree copy -r $A --from-path foo --to-path foo2 -m "subtree copy foo to foo2"
  copying foo to foo2
  $ hg subtree copy -r $A --from-path foo --to-path foo3 -m "subtree copy foo to foo3"
  copying foo to foo3
  $ hg subtree merge -r $B --from-path foo --to-path foo2
  computing merge base (timeout: 120 seconds)...
  merge base: b4cb27eee4e2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "merge B from foo to foo2"
  $ hg subtree merge -r $C --from-path foo --to-path foo3
  computing merge base (timeout: 120 seconds)...
  merge base: b4cb27eee4e2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "merge C from foo to foo3"
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  c90acfc6d9e6 merge C from foo to foo3
  │
  o  23b8d4a76647 merge B from foo to foo2
  │
  o  f6ef74a89a69 subtree copy foo to foo3
  │
  o  44d9b171824f subtree copy foo to foo2
  │
  o  8451df1af03b D
  │
  o  4701d37a062f C
  │
  o  c4fbbcdf676b B
  │
  o  b4cb27eee4e2 A
  $ hg histedit 23b8d4a76647 --commands - <<EOF
  > pick 23b8d4a76647 merge B from foo to foo2
  > f c90acfc6d9e6 merge C from foo to foo3
  > EOF
tofix: the combined commits should have two subtree merge metadata
  $ hg subtree inspect -r .
  {
    "merges": [
      {
        "version": 1,
        "from_commit": "c4fbbcdf676b67867d7a51393f12109974c5da59",
        "from_path": "foo",
        "to_path": "foo2"
      }
    ]
  }
