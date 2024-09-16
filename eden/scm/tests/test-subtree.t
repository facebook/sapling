  $ setconfig diff.git=True

setup backing repo

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS

  $ hg go $B -q

test subtree copy paths validation
  $ hg subtree copy -r $A
  abort: must provide --from-path and --to-path
  [255]
  $ hg subtree copy -r $A --from-path foo
  abort: must provide same number of --from-path and --to-path
  [255]
  $ hg subtree copy -r $A --from-path bar
  abort: must provide same number of --from-path and --to-path
  [255]
  $ hg subtree copy -r $A --from-path foo --to-path bar --from-path foo --to-path ""
  abort: overlapping --to-path entries
  [255]
  $ hg subtree copy -r $A --from-path nonexist --to-path bar
  abort: path 'nonexist' does not exist in commit d908813f0f7c
  [255]

test subtree copy
  $ hg subtree cp -r $A --from-path foo --to-path bar -m "subtree copy foo -> bar"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  04b520c8a658 subtree copy foo -> bar
  │
  o  b9450a0e6ae4 B
  │
  o  d908813f0f7c A
  $ hg show --git
  commit:      04b520c8a658
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  description:
  subtree copy foo -> bar
  
  Subtree copy from d908813f0f7c9078810e26aad1e37bdb32013d4b
  - Copied path foo to bar
  
  
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

test subtree graft
  $ newclientrepo
  $ drawdag <<'EOS'
  > C   # C/foo/x = 1a\n2\n3a\n
  > |
  > B   # B/foo/x = 1a\n2\n3\n
  > |
  > A   # A/foo/x = 1\n2\n3\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $C -q
  $ hg subtree copy -r $B --from-path foo --to-path bar -m 'subtree copy foo -> bar'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg subtree graft -r $C
  abort: must provide --from-path and --to-path
  [255]
  $ hg subtree graft -r $C --from-path foo
  abort: must provide --from-path and --to-path
  [255]
  $ hg subtree graft -r $C --to-path bar
  abort: must provide --from-path and --to-path
  [255]

  $ hg subtree graft -r $C --from-path foo --to-path bar
  grafting 78072751cf70 "C"
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  0eda80132ca9 C
  │
  o  e201af41baa5 subtree copy foo -> bar
  │
  o  78072751cf70 C
  │
  o  55ff286fb56f B
  │
  o  2f10237b4399 A
  $ hg show
  commit:      0eda80132ca9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/x
  description:
  C
  
  Grafted from 78072751cf70f1ca47671c625f3b2d7f86f45f00
  - Grafted path foo to bar
  
  
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,3 @@
   1a
   2
  -3
  +3a


test subtree merge
  $ newclientrepo
  $ drawdag <<'EOS'
  > D   # D/foo/x = 1a\n2\n3a\n4\n
  > |
  > C   # C/foo/x = 1a\n2\n3a\n
  > |
  > B   # B/foo/x = 1a\n2\n3\n
  > |
  > A   # A/foo/x = 1\n2\n3\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $D -q
  $ hg subtree copy -r $B --from-path foo --to-path bar -m 'subtree copy foo -> bar'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg subtree merge -r $D --from-path foo --to-path bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M bar/x
  $ hg diff
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,4 @@
   1a
   2
  -3
  +3a
  +4
  $ hg ci -m 'subtree merge foo to bar'
  $ hg show
  commit:      590261cf98e2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/x
  description:
  subtree merge foo to bar
  
  
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,4 @@
   1a
   2
  -3
  +3a
  +4

test subtree merge with normal copy
  $ newclientrepo
  $ drawdag <<'EOS'
  > C   # C/foo/x = 1a\n2\n3\n
  > |
  > B   # B/bar/x = 1\n2\n3\n (copied from foo/x)
  > |
  > A   # A/foo/x = 1\n2\n3\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $C -q
  $ hg subtree merge -r $C --from-path foo --to-path bar
  merging bar/x and foo/x to bar/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M bar/x
  $ hg diff
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,3 @@
  -1
  +1a
   2
   3

test subtree merge with no copy
  $ newclientrepo
  $ drawdag <<'EOS'
  > C   # C/foo/x = 1a\n2\n3\n
  > |
  > B   # B/bar/x = 1\n2\n3\n
  > |
  > A   # A/foo/x = 1\n2\n3\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $C -q
  $ hg subtree merge -r $C --from-path foo --to-path bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M bar/x
  $ hg diff
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,3 @@
  -1
  +1a
   2
   3
  
test subtree merge with no common base
  $ newclientrepo
  $ drawdag <<'EOS'
  > C    # D/bar/x = 1\n2\n3\n 
  > |    # C/foo/x = 1a\n2\n3a\n
  > B    # B/foo/x = 1a\n2\n3\n
  > |
  > A  D # A/foo/x = 1\n2\n3\n
  >      # drawdag.defaultfiles=false
  > EOS
  $ hg go $D -q
  $ hg log -G -T '{node|short} {desc}\n'
  o  78072751cf70 C
  │
  o  55ff286fb56f B
  │
  │ @  19915b669dd5 D
  │
  o  2f10237b4399 A
  $ zzl=1 hg subtree merge -r $C --from-path foo --to-path bar
  merging bar/x and foo/x to bar/x
  warning: 1 conflicts while merging bar/x! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg diff
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,9 @@
  +<<<<<<< working copy: 19915b669dd5 - test: D
   1
   2
   3
  +=======
  +1a
  +2
  +3a
  +>>>>>>> merge rev:    78072751cf70 - test: C
