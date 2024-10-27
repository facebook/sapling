  $ setconfig diff.git=True
  $ setconfig subtree.copy-reuse-tree=False
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
  rebasing 53a64b86e21e "subtree copy foo to foo2"
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'rebase_source': '53a64b86e21e09413fee85ae45ae94218c365e87', 'test_subtree_copy': '{"branches":[{"from_commit":"b4cb27eee4e2633aae0d62de87523007d1b5bfdd","from_path":"foo","to_path":"foo2"}],"v":1}'}

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
  rebasing 53a64b86e21e "subtree copy foo to foo2"
  abort: subtree copy dest path 'foo2' of '53a64b86e21e' has been updated on the other side
  (use 'hg subtree copy' to re-create the directory branch. Please see https://abc.com/sapling-subtree-copy-conflict for more help)
  [255]
