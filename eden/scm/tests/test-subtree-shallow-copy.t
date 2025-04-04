  $ setconfig diff.git=True
  $ setconfig subtree.allow-any-source-commit=True
  $ cat > $TESTTMP/subtree.py <<EOF
  > from sapling.commands import subtree
  > def extsetup(ui):
  >     subtree.COPY_REUSE_TREE = True
  > EOF
  $ setconfig extensions.subtreecopyreusetree=$TESTTMP/subtree.py

test subtree copy
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $B -q
  $ hg subtree cp -r $A --from-path foo --to-path bar -m "subtree copy foo -> bar"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'test_subtree': '[{"copies":[{"from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b","from_path":"foo","to_path":"bar"}],"v":1}]'}
  $ hg dbsh -c 'print(repo["."].changeset().files)'
  ()

files list is still empty after amending the shallow copy commit
  $ echo ccc >> bar/x
  $ hg amend
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'amend_source': '075709eca377ab1f8a1e6a31b7970d26ff9ec935', 'test_subtree': '[{"copies":[{"from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b","from_path":"foo","to_path":"bar"}],"v":1}]'}
  $ hg dbsh -c 'print(repo["."].changeset().files)'
  ()
  $ hg dbsh -c 'print(repo["."].files())'
  ['bar/x']
