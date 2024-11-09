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

test rebase subtree copy commit does not merge source changes
  $ newclientrepo
  $ drawdag <<'EOS'
  > C    # C/foo/y = 1'\n2\n3\n
  > |
  > B    # B/foo/y = 1\n2\n3\n
  > |
  > A    # A/foo/x = 1\n2\n3\n
  >      # A/foo2/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ ls foo2
  x
  $ hg subtree copy -r $B --from-path foo --to-path foo2 -m "subtree copy foo to foo2" --force
  removing foo2/x
  copying foo to foo2
  $ ls foo2
  x
  y
  $ hg rebase -r . -d $C
  rebasing deed6081d684 "subtree copy foo to foo2"
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  89c51647a56e subtree copy foo to foo2
  │
  o  af4c8c7579ca C
  │
  o  78fd32924038 B
  │
  o  de0c4b853cce A
  $ hg show
  commit:      89c51647a56e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo2/y
  description:
  subtree copy foo to foo2
  
  Subtree copy from 78fd32924038aebbed00334bd862a084d877e987
  - Copied path foo to foo2
  
  
  diff --git a/foo2/y b/foo2/y
  new file mode 100644
  --- /dev/null
  +++ b/foo2/y
  @@ -0,0 +1,3 @@
  +1
  +2
  +3

test rebase subtree copy commit does not introduce partial changes
  $ newclientrepo
  $ drawdag <<'EOS'
  > C    # C/foo/y = 1'\n2\n3\n
  > |    # C/foo/x = 1'\n2\n3\n
  > B    # B/foo/y = 1\n2\n3\n
  > |
  > A    # A/foo/x = 1\n2\n3\n
  >      # A/foo2/x = 1\n2\n3\n
  > EOS
  $ hg go -q $B
  $ ls foo2
  x
  $ hg subtree copy -r $B --from-path foo --to-path foo2 -m "subtree copy foo to foo2" --force
  removing foo2/x
  copying foo to foo2
  $ ls foo2
  x
  y
  $ hg rebase -r . -d $C
  rebasing deed6081d684 "subtree copy foo to foo2"
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  6f52bb96c2d6 subtree copy foo to foo2
  │
  o  cb9362f65bb1 C
  │
  o  78fd32924038 B
  │
  o  de0c4b853cce A
  $ hg show
  commit:      6f52bb96c2d6
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo2/y
  description:
  subtree copy foo to foo2
  
  Subtree copy from 78fd32924038aebbed00334bd862a084d877e987
  - Copied path foo to foo2
  
  
  diff --git a/foo2/y b/foo2/y
  new file mode 100644
  --- /dev/null
  +++ b/foo2/y
  @@ -0,0 +1,3 @@
  +1
  +2
  +3
