
setup backing repo

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS

test subtree copy
  $ hg go $B -q
  $ hg subtree cp -r $A --from-path foo --to-path bar -m "subtree copy foo -> bar"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  bfc51ae2a942 subtree copy foo -> bar
  │
  o  b9450a0e6ae4 B
  │
  o  d908813f0f7c A
  $ hg show --git
  commit:      bfc51ae2a942
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  description:
  subtree copy foo -> bar
  
  Subtree copy from d908813f0f7c9078810e26aad1e37bdb32013d4b
    Copied path foo to bar
  
  
  diff --git a/bar/x b/bar/x
  new file mode 100644
  --- /dev/null
  +++ b/bar/x
  @@ -0,0 +1,1 @@
  +aaa
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'test_branch_info': '{"v":1,"branches":[{"from_path":"foo","to_path":"bar","from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b"}]}'}


abort when the working copy is dirty

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS  
  $ hg go $B -q
  $ echo bbb >> foo/x
  $ hg st
  M foo/x
  $ hg subtree cp -r $A --from-path foo --to-path bar
  abort: uncommitted changes
  [255]
