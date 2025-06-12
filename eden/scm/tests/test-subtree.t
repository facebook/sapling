  $ setconfig diff.git=True
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1

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
  abort: must provide same number of --from-path ['foo'] and --to-path []
  [255]
  $ hg subtree copy -r $A --from-path bar
  abort: must provide same number of --from-path ['bar'] and --to-path []
  [255]
  $ hg subtree copy -r $A --from-path foo --to-path bar --from-path foo --to-path ""
  abort: overlapping --to-path entries
  [255]
  $ hg subtree copy -r $A --from-path nonexist --to-path bar
  abort: path 'nonexist' does not exist in commit d908813f0f7c
  [255]

test subtree copy source commit validation
  $ hg subtree cp -r $A --from-path foo --to-path bar --config subtree.allow-any-source-commit=False
  subtree copy from a non-public commit is not recommended. However, you can
  still proceed and use subtree copy and merge for common cases.
  (hint: see 'hg help subtree' for the impacts on subtree merge and log)
  Continue with subtree copy (y/n)?  n
  abort: subtree copy from a non-public commit is not allowed
  [255]
  $ hg subtree cp -r $A --from-path foo --to-path bar --config subtree.allow-any-source-commit=False --config subtree.education-page=https://abc.com/subtree
  subtree copy from a non-public commit is not recommended. However, you can
  still proceed and use subtree copy and merge for common cases.
  (hint: see subtree copy at https://abc.com/subtree for the impacts on subtree merge and log)
  Continue with subtree copy (y/n)?  n
  abort: subtree copy from a non-public commit is not allowed
  [255]

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
  copying foo to bar
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  255379dc5cbd subtree copy foo -> bar
  │
  o  b9450a0e6ae4 B
  │
  o  d908813f0f7c A
  $ hg show --git
  commit:      255379dc5cbd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/x
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
  {'branch': 'default', 'test_subtree': '[{"deepcopies":[{"from_commit":"d908813f0f7c9078810e26aad1e37bdb32013d4b","from_path":"foo","to_path":"bar"}],"v":1}]'}

test subtree copy metadata sorted by to-path
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # A/foo2/y = yyy\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $B -q
  $ hg subtree cp -r $A --from-path foo --to-path bar --from-path foo2 --to-path baa -m "subtree copy foo -> bar and foo2 -> baa"
  copying foo to bar
  copying foo2 to baa
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  c37cc3582a27 subtree copy foo -> bar and foo2 -> baa
  │
  o  8782b677794f B
  │
  o  4c412676b7b9 A
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'test_subtree': '[{"deepcopies":[{"from_commit":"4c412676b7b9698f29843de329a5c3b654034990","from_path":"foo2","to_path":"baa"},{"from_commit":"4c412676b7b9698f29843de329a5c3b654034990","from_path":"foo","to_path":"bar"}],"v":1}]'}

test subtree copy without skipping source commit check: new commit does not have subtree metadata
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $B -q
  $ setconfig ui.interactive=True
  $ setconfig subtree.allow-any-source-commit=False
  $ hg subtree cp -r $A --from-path foo --to-path bar -m "subtree copy foo -> bar"<<EOF
  > y
  > EOF
  subtree copy from a non-public commit is not recommended. However, you can
  still proceed and use subtree copy and merge for common cases.
  (hint: see 'hg help subtree' for the impacts on subtree merge and log)
  Continue with subtree copy (y/n)?  y
  copying foo to bar
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  b123ad7c241c subtree copy foo -> bar
  │
  o  b9450a0e6ae4 B
  │
  o  d908813f0f7c A
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default'}

abort when subtree copy too many files

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # A/foo/y = yyy\n
  >     # drawdag.defaultfiles=false
  > EOS  
  $ hg subtree cp -r $A --from-path foo --to-path bar --config subtree.max-file-count=1
  abort: path 'foo' includes too many files: 2 (max: 1)
  [255]
  $ hg subtree cp -r $A --from-path foo --to-path bar --config subtree.max-file-count=1 --config ui.supportcontact="Sapling Team"
  abort: path 'foo' includes too many files: 2 (max: 1)
  (contact Sapling Team for help)
  [255]

test max file count for multiple from paths, check each from path separately

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # A/foo/y = yyy\n
  >     # A/bar/z = zzz\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg subtree cp -r $A --from-path bar --to-path bar2 --from-path foo --to-path foo2 --config subtree.max-file-count=1
  abort: path 'foo' includes too many files: 2 (max: 1)
  [255]
  $ hg subtree cp -r $A --from-path bar --to-path bar2 --from-path foo --to-path foo2 --config subtree.max-file-count=2
  copying bar to bar2
  copying foo to foo2

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

test subtree copy with symlinks
  $ newclientrepo
  $ mkdir foo
  $ echo "aaa" > foo/a
  $ ln -s a foo/b
  $ hg ci -Aqm 'first'
  $ echo "bbb" > foo/a
  $ hg ci -m 'second'
  $ hg subtree cp -r "desc(first)" --from-path foo --to-path foo2
  copying foo to foo2
  $ readlink foo2/b
  a
  $ cat foo2/b
  aaa

#if execbit
test subtree copy with execs
  $ newclientrepo
  $ mkdir foo
  $ echo "aaa" > foo/a
  $ chmod +x foo/a
  $ echo "bbb" > foo/b
  $ hg ci -Aqm 'first'
  $ echo "bbb" > foo/a
  $ hg ci -m 'second'
  $ hg subtree cp -r "desc(first)" --from-path foo --to-path foo2
  copying foo to foo2
  $ f -m foo/a foo/b foo2/a foo2/b
  foo/a: mode=755
  foo/b: mode=644
  foo2/a: mode=755
  foo2/b: mode=644
  $ hg dbsh -c 'for x in ["foo/a", "foo/b", "foo2/a", "foo2/b"]: print([x, repo["."].manifest().flags(x)])'
  ['foo/a', 'x']
  ['foo/b', '']
  ['foo2/a', 'x']
  ['foo2/b', '']
#endif

test subtree copy to tracked directory
  $ newclientrepo
  $ drawdag <<'EOS'
  > C   # B/bar/x = ccc\n
  > |
  > B   # B/foo/x = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS  
  $ hg go $C -q
  $ hg subtree cp -r $A --from-path foo --to-path bar
  abort: cannot copy to an existing path: bar
  (use --force to overwrite)
  [255]
  $ hg subtree cp -r $A --from-path foo --to-path bar --force
  removing bar/x
  copying foo to bar
  $ cat bar/x
  aaa

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
  copying foo to bar

  $ hg subtree graft -r $C
  abort: must provide --from-path and --to-path for same-repo grafts
  [255]
  $ hg subtree graft -r $C --from-path foo
  abort: must provide --from-path and --to-path for same-repo grafts
  [255]
  $ hg subtree graft -r $C --to-path bar
  abort: must provide --from-path and --to-path for same-repo grafts
  [255]

  $ hg subtree graft -r $C --from-path foo --to-path bar
  grafting 78072751cf70 "C"
  merging bar/x and foo/x to bar/x
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  4e260db10c28 Graft "C"
  │
  o  5e3aa22b08c2 subtree copy foo -> bar
  │
  o  78072751cf70 C
  │
  o  55ff286fb56f B
  │
  o  2f10237b4399 A
  $ hg show
  commit:      4e260db10c28
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/x
  description:
  Graft "C"
  
  Grafted 78072751cf70f1ca47671c625f3b2d7f86f45f00
  - Grafted foo to bar
  
  
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,3 @@
   1a
   2
  -3
  +3a

test 'subtree graft -m'
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
  copying foo to bar

  $ hg subtree graft -r $C --from-path foo --to-path bar -m "new C"
  grafting 78072751cf70 "C"
  merging bar/x and foo/x to bar/x
  $ hg show
  commit:      6592d0497ef7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/x
  description:
  new C
  
  Grafted 78072751cf70f1ca47671c625f3b2d7f86f45f00
  - Grafted foo to bar
  
  
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,3 @@
   1a
   2
  -3
  +3a

test 'subtree graft -m' with test plan
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
  copying foo to bar

  $ hg subtree graft -r $C --from-path foo --to-path bar -m "new C\
  > \
  > Test Plan:\
  > \
  > test 123"
  grafting 78072751cf70 "C"
  merging bar/x and foo/x to bar/x
  $ hg show
  commit:      a92251181d99
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/x
  description:
  new C
  
  Grafted 78072751cf70f1ca47671c625f3b2d7f86f45f00
  - Grafted foo to bar
  
  Test Plan:
  
  test 123
  
  
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,3 @@
   1a
   2
  -3
  +3a

Test 'subtree graft -m' with --no-log
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
  copying foo to bar

  $ hg subtree graft --no-log -r $C --from-path foo --to-path bar -m "new C\
  > \
  > Test Plan:\
  > \
  > test 123"
  grafting 78072751cf70 "C"
  merging bar/x and foo/x to bar/x
  $ hg show
  commit:      9a63dfba4f06
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/x
  description:
  new C
  
  Test Plan:
  
  test 123
  
  
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
  copying foo to bar
  $ hg subtree merge -r $D --from-path foo --to-path bar
  searching for merge base ...
  found the last subtree copy commit 0f9175ec4003
  merge base: 55ff286fb56f
  merging bar/x and foo/x to bar/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
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
  $ hg continue
  abort: cannot continue with 'hg commit' in non-interactive mode
  (use 'hg commit' to commit or 'hg status' for more info)
  [255]
  $ hg ci -m 'subtree merge foo to bar'
  $ hg dbsh -c 'print(repo["."].extra())'
  {'branch': 'default', 'test_subtree': '[{"merges":[{"from_commit":"907442010f516d83aea80b4382964be22a34214f","from_path":"foo","to_path":"bar"}],"v":1}]'}
  $ hg show
  commit:      4efbbbca7984
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/x
  description:
  subtree merge foo to bar
  
  Subtree merge from 907442010f516d83aea80b4382964be22a34214f
  - Merged path foo to bar
  
  
  diff --git a/bar/x b/bar/x
  --- a/bar/x
  +++ b/bar/x
  @@ -1,3 +1,4 @@
   1a
   2
  -3
  +3a
  +4
should have one parent
  $ hg log -r . -T '{parents}'
  0f9175ec4003  (no-eol)
  $ hg log bar
  commit:      4efbbbca7984
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     subtree merge foo to bar
  
  commit:      0f9175ec4003
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     subtree copy foo -> bar

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
  searching for merge base ...
  merge base: 2f10237b4399
  merging bar/x and foo/x to bar/x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg st
  M bar/x
  $ hg debugmergestate | grep -B 1 -A 2 "subtree merges"
  other: df87606c27154ec2ea14aac8fd294e2a611a2a82
  subtree merges:
    from_commit: df87606c27154ec2ea14aac8fd294e2a611a2a82, from: foo, to: bar
  labels:
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
  searching for merge base ...
  merge base: 2f10237b4399
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
  $ hg subtree merge -r $C --from-path foo --to-path bar
  searching for merge base ...
  merge base: 19915b669dd5
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
