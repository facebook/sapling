  $ setconfig diff.git=True
  $ setconfig subtree.copy-reuse-tree=False
  $ setconfig subtree.allow-any-source-commit=True
  $ enable rebase

test rebase subtree copy commit and keep the subtree copy metadata
  $ newclientrepo
  $ drawdag <<'EOS'
  > B C  # B/foo/x = 1a\n2\n3\n
  > |/   # C/foo/x = 1\n2\n3a\n
  > A    # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $C
  $ hg subtree copy -r $A --from-path foo --to-path foo2 -m "subtree copy foo to foo2"
  copying foo to foo2
  $ hg rebase -r . -d $B
  rebasing dad7999e558f "subtree copy foo to foo2"
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'rebase_source': 'dad7999e558f896af0f4e032eca26ecc5de27ed8', 'test_subtree': '[{"deepcopies":[{"from_commit":"b4cb27eee4e2633aae0d62de87523007d1b5bfdd","from_path":"foo","to_path":"foo2"}],"v":1}]'}

test rebase subtree copy commit fails if the to-path is updated on the dest side
  $ newclientrepo
  $ drawdag <<'EOS'
  > B C  # B/foo2/x = 1a\n2\n3\n
  > |/   # C/foo/x = 1\n2\n3a\n
  > A    # A/foo/x = 1\n2\n3\n
  > EOS
  $ hg go -q $C
  $ hg subtree copy -r $A --from-path foo --to-path foo2 -m "subtree copy foo to foo2"
  copying foo to foo2
  $ setconfig subtree.copy-conflict-hint="Please see https://abc.com/sapling-subtree-copy-conflict for more help"
  $ hg rebase -r . -d $B
  rebasing dad7999e558f "subtree copy foo to foo2"
  abort: subtree copy dest path 'foo2' of 'dad7999e558f' has been updated on the other side
  (use 'hg subtree copy' to re-create the directory branch. Please see https://abc.com/sapling-subtree-copy-conflict for more help)
  [255]
