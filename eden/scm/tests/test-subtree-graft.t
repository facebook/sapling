  $ setconfig diff.git=true
  $ enable morestatus
  $ setconfig morestatus.show=true
  $ setconfig drawdag.defaultfiles=false

Test validation of --from-path and --to-path
  $ newclientrepo
  $ echo "A" | drawdag
  $ hg subtree graft -r $A --from-path foo --to-path bar --from-path foo2
  grafting 7b3f3d5e5faf "A"
  abort: must provide same number of --from-path ['foo', 'foo2'] and --to-path ['bar']
  [255]
  $ hg subtree graft -r $A --from-path foo --to-path bar --to-path bar2
  grafting 7b3f3d5e5faf "A"
  abort: must provide same number of --from-path ['foo'] and --to-path ['bar', 'bar2']
  [255]
  $ hg subtree graft -r $A --from-path foo --from-path bar --to-path baz --to-path baz/qux
  grafting 7b3f3d5e5faf "A"
  abort: overlapping --to-path entries
  [255]
  $ hg subtree graft -r $A --from-path foo --from-path bar --to-path baz --to-path ""
  grafting 7b3f3d5e5faf "A"
  abort: overlapping --to-path entries
  [255]
  $ hg subtree graft -r $A --from-path foo --from-path bar --to-path baz/a --to-path baz/b
  grafting 7b3f3d5e5faf "A"
  path 'foo' does not exist in commit 7b3f3d5e5faf
  path 'bar' does not exist in commit 7b3f3d5e5faf
  note: graft of 7b3f3d5e5faf created no changes to commit
  $ hg subtree graft -r $A --from-path foo --from-path bar --to-path baz/a --to-path baz/a
  grafting 7b3f3d5e5faf "A"
  abort: overlapping --to-path entries
  [255]


Basic case merging a file change between directory branches "foo" and "bar".
  $ newclientrepo
  $ drawdag <<EOS
  > C B  # B/bar/file = a\nb\ncc\n (copied from foo/file)
  > |/   # C/foo/file = aa\nb\nc\n
  > A    # A/foo/file = a\nb\nc\n
  > EOS
  $ hg go -q $B
  $ hg subtree graft -qr $C --from-path foo --to-path bar
  $ hg show
  commit:      46fdedefab05
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file
  description:
  Graft "C"
  
  Grafted 09a920923fbb29a6c9977eae526b1730d53c9be6
  - Grafted foo to bar
  
  
  diff --git a/bar/file b/bar/file
  --- a/bar/file
  +++ b/bar/file
  @@ -1,3 +1,3 @@
  -a
  +aa
   b
   cc


Graft a commit adding a new file:
  $ newclientrepo
  $ drawdag <<EOS
  > C B  # A/foo/file = file\n
  > |/   # B/bar/file = file\n (copied from foo/file)
  > A    # C/foo/new = new\n
  > EOS
  $ hg go -q $B
  $ hg st
  $ hg subtree graft -qr $C --from-path foo --to-path bar
  $ hg show
  commit:      b8302edf7170
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/new
  description:
  Graft "C"
  
  Grafted b7298624ac858378b6227152febcc313c3bfb348
  - Grafted foo to bar
  
  
  diff --git a/bar/new b/bar/new
  new file mode 100644
  --- /dev/null
  +++ b/bar/new
  @@ -0,0 +1,1 @@
  +new


Graft a commit deleting a file:
  $ newclientrepo
  $ drawdag <<EOS
  > B    # B/bar/file = (removed)
  > |
  > A    # A/foo/file = file\n
  >      # A/bar/file = file\n
  > EOS
  $ hg go -q $A
  $ hg subtree graft -qr $B --from-path bar --to-path foo
  $ hg show
  commit:      3515137c44d8
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/file
  description:
  Graft "B"
  
  Grafted cf6063bb81125c62e42fd1040b2490659e503e3b
  - Grafted bar to foo
  
  
  diff --git a/foo/file b/foo/file
  deleted file mode 100644
  --- a/foo/file
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -file


Graft a file that was renamed in dest branch:
  $ newclientrepo
  $ drawdag <<EOS
  >   D  # D/bar/rename = a\nb\ncc\n (renamed from bar/file)
  >   |
  > C B  # A/foo/file = a\nb\nc\n
  > |/   # B/bar/file = a\nb\ncc\n (copied from foo/file)
  > A    # C/foo/file = aa\nb\nc\n
  > EOS
  $ hg go -q $D
  $ hg subtree graft -qr $C --from-path foo --to-path bar
  $ hg show
  commit:      2aa9f1f57629
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/rename
  description:
  Graft "C"
  
  Grafted 09a920923fbb29a6c9977eae526b1730d53c9be6
  - Grafted foo to bar
  
  
  diff --git a/bar/rename b/bar/rename
  --- a/bar/rename
  +++ b/bar/rename
  @@ -1,3 +1,3 @@
  -a
  +aa
   b
   cc


Graft a commit renaming a file:
  $ newclientrepo
  $ drawdag <<EOS
  > C B  # B/bar/file = a\nb\ncc\n (copied from foo/file)
  > |/   # C/foo/rename = aa\nb\nc\n (renamed from foo/file)
  > A    # A/foo/file = a\nb\nc\n
  > EOS
  $ hg go -q $B
  $ hg subtree graft -qr $C --from-path foo --to-path bar
  $ hg show
  commit:      0b47c9e1117d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file bar/rename
  description:
  Graft "C"
  
  Grafted 53d1a0c140f97ab323b0d4a1acefa7ed74604e71
  - Grafted foo to bar
  
  
  diff --git a/bar/file b/bar/rename
  rename from bar/file
  rename to bar/rename
  --- a/bar/file
  +++ b/bar/rename
  @@ -1,3 +1,3 @@
  -a
  +aa
   b
   cc

Graft a commit with rename in "remote" history:
  $ newclientrepo
  $ drawdag <<EOS
  > D    # D/foo/rename = aa\nb\nc\n
  > |
  > C B  # B/bar/file = a\nb\ncc\n
  > |/   # C/foo/rename = a\nb\nc\n (renamed from foo/file)
  > A    # A/foo/file = a\nb\nc\n
  > EOS
  $ hg go -q $B
  $ hg subtree graft -qr $D --from-path foo --to-path bar
  $ hg show
  commit:      d1b5d43e386f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file
  description:
  Graft "D"
  
  Grafted f474dcdb45f7579c1ab82a5cfdab40525db086df
  - Grafted foo to bar
  
  
  diff --git a/bar/file b/bar/file
  --- a/bar/file
  +++ b/bar/file
  @@ -1,3 +1,3 @@
  -a
  +aa
   b
   cc


Graft a commit with rename in "local" history:
  $ newclientrepo
  $ drawdag <<EOS
  > D E  # D/foo/rename = aa\nb\nc\n
  > | |  # E/bar/file = a\nb\ncc\n
  > C B  # B/bar/file = a\nb\nc\n
  > |/   # C/foo/rename = a\nb\nc\n (renamed from foo/file)
  > A    # A/foo/file = a\nb\nc\n
  > EOS
  $ hg go -q $D
  $ hg subtree graft -qr $E --from-path bar --to-path foo
  $ hg show
  commit:      851154b50e17
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/rename
  description:
  Graft "E"
  
  Grafted 18e3512650fdf23ebbcf589607dbd700602bee93
  - Grafted bar to foo
  
  
  diff --git a/foo/rename b/foo/rename
  --- a/foo/rename
  +++ b/foo/rename
  @@ -1,3 +1,3 @@
   aa
   b
  -c
  +cc


Graft a commit with renames on both sides:
  $ newclientrepo
  $ drawdag <<EOS
  >   F  # F/bar/rename2 = a\nb\ncc\n
  >   |
  > D E  # D/foo/rename = aa\nb\nc\n
  > | |  # E/bar/rename2 = a\nb\nc\n (renamed from bar/file)
  > C B  # B/bar/file = a\nb\nc\n
  > |/   # C/foo/rename = a\nb\nc\n (renamed from foo/file)
  > A    # A/foo/file = a\nb\nc\n
  > EOS
  $ hg go -q $D
  $ hg subtree graft -qr $F --from-path bar --to-path foo
  $ hg show
  commit:      fbf67aa170e0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/rename
  description:
  Graft "F"
  
  Grafted 7d4e96ab007e943d7bafd40a5aa67cf493c5d818
  - Grafted bar to foo
  
  
  diff --git a/foo/rename b/foo/rename
  --- a/foo/rename
  +++ b/foo/rename
  @@ -1,3 +1,3 @@
   aa
   b
  -c
  +cc


Grafting individual files also works:
  $ newclientrepo
  $ drawdag <<EOS
  >   C  # C/B = aa\nb\nc\n
  >   |
  > D B  # D/A = a\nb\ncc\n
  > |/   # B/B = a\nb\nc\n (copied from A)
  > A    # A/A = a\nb\nc\n
  > EOS
  $ hg go -q $D
  $ hg subtree graft -qr $C --from-path B --to-path A
  $ hg show
  commit:      9f5242fa10f8
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A
  description:
  Graft "C"
  
  Grafted ea0e3d741c410c6984853baacef718860cfc18a5
  - Grafted B to A
  
  
  diff --git a/A b/A
  --- a/A
  +++ b/A
  @@ -1,3 +1,3 @@
  -a
  +aa
   b
   cc


Can graft between completely unrelated directories:
  $ newclientrepo
  $ drawdag <<EOS
  > B  # B/A = a\nb\ncc\n
  > |
  > A  # A/A = a\nb\nc\n
  > 
  > C  # C/C = aa\nb\nc\n
  > EOS
  $ hg go -q $C
  $ hg subtree graft -qr $B --from-path A --to-path C
  $ hg show
  commit:      16df3ad98c14
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       C
  description:
  Graft "B"
  
  Grafted eb8f2e58912725da3773edc0e24d884469f2bb1c
  - Grafted A to C
  
  
  diff --git a/C b/C
  --- a/C
  +++ b/C
  @@ -1,3 +1,3 @@
   aa
   b
  -c
  +cc


Can do multiple mappings in a single graft:
  $ newclientrepo
  $ drawdag <<EOS
  > D  # D/dir/file = AA\n
  > |
  > C  # C/dir3/file = A\n
  > |
  > B  # B/dir2/file = A\n
  > |
  > A  # A/dir/file = A\n
  > EOS
  $ hg go -q $C
  $ hg subtree graft -qr $D --from-path dir --to-path dir2 --from-path dir --to-path dir3
  $ hg show
  commit:      f5ae7c01cd83
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dir2/file dir3/file
  description:
  Graft "D"
  
  Grafted 08771e12ccbd5547592676e9a972caafcd7b0820
  - Grafted dir to dir2
  - Grafted dir to dir3
  
  
  diff --git a/dir2/file b/dir2/file
  --- a/dir2/file
  +++ b/dir2/file
  @@ -1,1 +1,1 @@
  -A
  +AA
  diff --git a/dir3/file b/dir3/file
  --- a/dir3/file
  +++ b/dir3/file
  @@ -1,1 +1,1 @@
  -A
  +AA


Multiple mappings can all follow renames:
  $ newclientrepo
  $ drawdag <<EOS
  > G  # G/dir/rename = AA\n
  > |
  > F  # F/dir/rename = A\n (renamed from dir/file)
  > |
  > E  # E/dir3/rename3 = A\n (renamed from dir3/file)
  > |
  > D  # D/dir3/file = A\n
  > |
  > C  # C/dir2/rename2 = A\n (renamed from dir2/file)
  > |
  > B  # B/dir2/file = A\n
  > |
  > A  # A/dir/file = A\n
  > EOS
  $ hg go -q $G
  $ hg subtree graft -qr $G --from-path dir --to-path dir2 --from-path dir --to-path dir3
  $ hg show
  commit:      accc78a2e737
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dir2/rename2 dir3/rename3
  description:
  Graft "G"
  
  Grafted fab9c6fdbcd0fc0139ace494073efb5c40011ed1
  - Grafted dir to dir2
  - Grafted dir to dir3
  
  
  diff --git a/dir2/rename2 b/dir2/rename2
  --- a/dir2/rename2
  +++ b/dir2/rename2
  @@ -1,1 +1,1 @@
  -A
  +AA
  diff --git a/dir3/rename3 b/dir3/rename3
  --- a/dir3/rename3
  +++ b/dir3/rename3
  @@ -1,1 +1,1 @@
  -A
  +AA


Don't get confused by renames too far in the past on src side:
  $ newclientrepo
  $ drawdag <<EOS
  > F  # F/dir/rename3 = AA\n
  > |
  > E  # E/dir/rename3 = A\n (renamed from dir/rename2)
  > |
  > D  # D/dir2/rename2 = A\n
  > |
  > C  # C/dir/rename2 = A\n (renamed from dir/rename1)
  > |
  > B  # B/dir/rename1 = A\n (renamed from dir/file)
  > |
  > A  # A/dir/file = A\n
  > EOS
  $ hg go -q $E
  $ hg subtree graft -qr $F --from-path dir --to-path dir2
  $ hg show
  commit:      72b278f748ef
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dir2/rename2
  description:
  Graft "F"
  
  Grafted dacfc2aa45adb71c3c557083202bd9178b2e7485
  - Grafted dir to dir2
  
  
  diff --git a/dir2/rename2 b/dir2/rename2
  --- a/dir2/rename2
  +++ b/dir2/rename2
  @@ -1,1 +1,1 @@
  -A
  +AA


Trace rename history before directory branch point:
  $ newclientrepo
  $ drawdag <<EOS
  > E  # E/dir4/rename4 = AA\n
  > |
  > D  # D/dir4/rename4 = A\n (copied from dir/file)
  > |
  > C  # C/dir3/rename3 = A\n (copied from dir2/rename2)
  > |
  > B  # B/dir2/rename2 = A\n (copied from dir/file)
  > |
  > A  # A/dir/file = A\n
  > EOS
  $ hg go -q $E
TODO: we should be able to follow copies here once we have splice metadata
  $ hg subtree graft -qr $E --from-path dir4 --to-path dir3
  other [graft] changed dir3/rename4 which local [local] is missing
  hint: if this is due to a renamed file, you can manually input the renamed path
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]


Merge conflict - both sides modified:
  $ newclientrepo
  $ drawdag <<EOS
  > B    # B/foo/file = one\n
  > |    # B/bar/file = two\n
  > A    # A/foo/file = file\n
  >      # A/bar/file = file\n
  > EOS
  $ hg go -q $B
  $ hg subtree graft -qr $B --from-path foo --to-path bar
  warning: 1 conflicts while merging bar/file! (edit, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  $ hg st
  M bar/file
  ? bar/file.orig
  
  # The repository is in an unfinished *graft* state.
  # Unresolved merge conflicts (1):
  # 
  #     bar/file
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  # To continue:                hg graft --continue
  # To abort:                   hg graft --abort
  $ cat bar/file
  <<<<<<< local: dfb58fd2ac21 - test: B
  two
  =======
  one
  >>>>>>> graft: dfb58fd2ac21 - test: B
  $ echo "one\ntwo" > bar/file
  $ hg resolve --mark bar/file
  (no more unresolved files)
  continue: hg graft --continue
  $ hg subtree graft --continue
  grafting dfb58fd2ac21 "B"
  $ hg show
  commit:      87bdf3309275
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file
  description:
  Graft "B"
  
  Grafted dfb58fd2ac217c798317e8635e73d346568ceb29
  - Grafted foo to bar
  
  
  diff --git a/bar/file b/bar/file
  --- a/bar/file
  +++ b/bar/file
  @@ -1,1 +1,2 @@
  +one
   two


Merge conflict - delete/modified:
  $ newclientrepo
  $ drawdag <<EOS
  > B    # B/foo/file = (removed)
  > |    # B/bar/file = two\n
  > A    # A/foo/file = file\n
  >      # A/bar/file = file\n
  > EOS
  $ hg go -q $B
  $ hg subtree graft -qr $B --from-path foo --to-path bar
  local [local] changed bar/file which other [graft] deleted (as foo/file)
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  $ hg st
  
  # The repository is in an unfinished *graft* state.
  # Unresolved merge conflicts (1):
  # 
  #     bar/file
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  # To continue:                hg graft --continue
  # To abort:                   hg graft --abort
  $ hg rm bar/file
  $ hg resolve --mark bar/file
  (no more unresolved files)
  continue: hg graft --continue
  $ hg subtree graft --continue
  grafting a088319ec9f4 "B"
  $ hg show
  commit:      1f2bba140221
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file
  description:
  Graft "B"
  
  Grafted a088319ec9f4a6b98f1f4cbd6389b16f4c0141dc
  - Grafted foo to bar
  
  
  diff --git a/bar/file b/bar/file
  deleted file mode 100644
  --- a/bar/file
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -two


Can opt out of "Grafted by" line in commit message:
  $ newclientrepo
  $ drawdag <<EOS
  > B  # B/B = B\n (copied from A)
  > |
  > A  # A/A = A\n
  > EOS
  $ hg go -q $A
  $ hg subtree graft -qr $B --from-path B --to-path A --no-log
  $ hg show
  commit:      8041fbbca30f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A
  description:
  B
  
  
  diff --git a/A b/A
  --- a/A
  +++ b/A
  @@ -1,1 +1,1 @@
  -A
  +B


Cross-directory graft add graft info as summary footer:
  $ newclientrepo
  $ drawdag <<EOS
  > B  # B/B = B\n (copied from A)
  > |
  > A  # A/A = A\n
  > EOS
  $ hg metaedit -r $B -m "B\
  > \
  > Summary:\
  > \
  > Foo\
  > \
  > Test Plan:\
  > \
  > Bar"
  $ hg go -q $A
  $ hg subtree graft -qr 'desc("Summary")' --from-path B --to-path A --config extensions.fbcodereview=
  $ hg show
  commit:      1c778f05fddf
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A
  description:
  Graft "B"
  
  Summary:
  
  Foo
  
  Grafted e8470334d2058106534ac7d72485e6bfaa76ca01
  - Grafted B to A
  
  
  diff --git a/A b/A
  --- a/A
  +++ b/A
  @@ -1,1 +1,1 @@
  -A
  +B


Cross-directory graft removes phabricator tags (excerpt "Summary"):
  $ newclientrepo
  $ drawdag <<EOS
  > B  # B/B = B\n (copied from A)
  > |
  > A  # A/A = A\n
  > EOS
  $ hg metaedit -r $B -m "B\
  > \
  > Summary:\
  > \
  > Foo\
  > \
  > Test Plan:\
  > \
  > Bar\
  > \
  > Reviewed By: test1, test2\
  > \
  > Tags: tag1, tag2\
  > \
  > Differential Revision: example.com/D123"
  $ hg go -q $A
  $ hg subtree graft -qr 'desc("Differential")' --from-path B --to-path A --config extensions.fbcodereview=
  $ hg show
  commit:      534891799983
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A
  description:
  Graft "B"
  
  Summary:
  
  Foo
  
  Grafted 6a2f4620ac267db57400b129af01ac66f3cf2311 (D123)
  - Grafted B to A
  
  
  diff --git a/A b/A
  --- a/A
  +++ b/A
  @@ -1,1 +1,1 @@
  -A
  +B

Graft supports non-root relative paths
  $ newclientrepo
  $ drawdag <<EOS
  > C B  # B/my/bar/file = a\nb\ncc\n (copied from my/foo/file)
  > |/   # C/my/foo/file = aa\nb\nc\n
  > A    # A/my/foo/file = a\nb\nc\n
  > EOS
  $ hg go -q $B
  $ cd my
  $ hg subtree graft -qr $C --from-path foo --to-path bar
  $ hg show
  commit:      aa03702134a2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       my/bar/file
  description:
  Graft "C"
  
  Grafted 48b96237613e0f4a5fb16198b55dd4a03ca3c527
  - Grafted my/foo to my/bar
  
  
  diff --git a/my/bar/file b/my/bar/file
  --- a/my/bar/file
  +++ b/my/bar/file
  @@ -1,3 +1,3 @@
  -a
  +aa
   b
   cc

Can use --message-field to update parts of commit message.
  $ newclientrepo
  $ touch foo
  $ hg commit -Aqm "title
  > 
  > Summary:
  > summary
  > 
  > Test Plan:
  > test plan"
  $ hg whereami
  2d2e42cdf85511ef5011a5cf09d60e5c319d9e9b
  $ hg go -q null
  $ hg subtree graft -r 2d2e42cdf --from-path foo --to-path bar --message-field="Summary=
  > new summary
  > "
  grafting 2d2e42cdf855 "title"
  $ hg show
  commit:      5d37bc3ef25f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar
  description:
  title
  
  Summary:
  new summary
  
  Grafted 2d2e42cdf85511ef5011a5cf09d60e5c319d9e9b
  - Grafted foo to bar
  
  Test Plan:
  test plan
  
  
  diff --git a/bar b/bar
  new file mode 100644

Graft a commit should not result into a merge state when complete successfully
  $ newclientrepo
  $ drawdag <<EOS
  > C B  # B/bar/file = a\nb\ncc\n (copied from foo/file)
  > |/   # C/foo/rename = aa\nb\nc\n (renamed from foo/file)
  > A    # A/foo/file = a\nb\nc\n
  > EOS
  $ hg go -q $B
  $ hg subtree graft -r $C --from-path non-exist --to-path non-exist
  grafting 53d1a0c140f9 "C"
  path 'non-exist' does not exist in commit 53d1a0c140f9
  note: graft of 53d1a0c140f9 created no changes to commit
  $ hg st
