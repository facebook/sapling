  $ setconfig diff.git=true
  $ enable morestatus
  $ setconfig morestatus.show=true
  $ setconfig drawdag.defaultfiles=false

Test validation of --from-path and --to-path
  $ newclientrepo
  $ echo "A" | drawdag
  $ hg graft -r $A --from-path foo
  grafting 7b3f3d5e5faf "A"
  abort: must provide same number of --from-path and --to-path
  [255]
  $ hg graft -r $A --to-path foo
  grafting 7b3f3d5e5faf "A"
  abort: must provide same number of --from-path and --to-path
  [255]
  $ hg graft -r $A --from-path foo --from-path bar --to-path baz --to-path baz/qux
  grafting 7b3f3d5e5faf "A"
  abort: overlapping --to-path entries
  [255]
  $ hg graft -r $A --from-path foo --from-path bar --to-path baz --to-path ""
  grafting 7b3f3d5e5faf "A"
  abort: overlapping --to-path entries
  [255]
  $ hg graft -r $A --from-path foo --from-path bar --to-path baz/a --to-path baz/b
  grafting 7b3f3d5e5faf "A"
  path 'foo' does not exist in commit 7b3f3d5e5faf
  path 'bar' does not exist in commit 7b3f3d5e5faf
  note: graft of 7b3f3d5e5faf created no changes to commit
  $ hg graft -r $A --from-path foo --from-path bar --to-path baz/a --to-path baz/a
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
  $ hg graft -qr $C --from-path foo --to-path bar
  $ hg show
  commit:      a4bf043e97ca
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file
  description:
  C
  
  Grafted from 09a920923fbb29a6c9977eae526b1730d53c9be6
  - Grafted path foo to bar
  
  
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
  $ hg graft -qr $C --from-path foo --to-path bar
  $ hg show
  commit:      73f800881fa6
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/new
  description:
  C
  
  Grafted from b7298624ac858378b6227152febcc313c3bfb348
  - Grafted path foo to bar
  
  
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
  $ hg graft -qr $B --from-path bar --to-path foo
  $ hg show
  commit:      fb88b5cc8dcd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/file
  description:
  B
  
  Grafted from cf6063bb81125c62e42fd1040b2490659e503e3b
  - Grafted path bar to foo
  
  
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
  $ hg graft -qr $C --from-path foo --to-path bar
  $ hg show
  commit:      45ada74c01d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/rename
  description:
  C
  
  Grafted from 09a920923fbb29a6c9977eae526b1730d53c9be6
  - Grafted path foo to bar
  
  
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
  $ hg graft -qr $C --from-path foo --to-path bar
  $ hg show
  commit:      5a4738920578
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file bar/rename
  description:
  C
  
  Grafted from 53d1a0c140f97ab323b0d4a1acefa7ed74604e71
  - Grafted path foo to bar
  
  
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
  $ hg graft -qr $D --from-path foo --to-path bar
  $ hg show
  commit:      ea8341b07380
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file
  description:
  D
  
  Grafted from f474dcdb45f7579c1ab82a5cfdab40525db086df
  - Grafted path foo to bar
  
  
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
  $ hg graft -qr $E --from-path bar --to-path foo
  $ hg show
  commit:      b0f4979359c5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/rename
  description:
  E
  
  Grafted from 18e3512650fdf23ebbcf589607dbd700602bee93
  - Grafted path bar to foo
  
  
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
  $ hg graft -qr $F --from-path bar --to-path foo
  $ hg show
  commit:      da526f4b3b28
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/rename
  description:
  F
  
  Grafted from 7d4e96ab007e943d7bafd40a5aa67cf493c5d818
  - Grafted path bar to foo
  
  
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
  $ hg graft -qr $C --from-path B --to-path A
  $ hg show
  commit:      4614d505f924
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A
  description:
  C
  
  Grafted from ea0e3d741c410c6984853baacef718860cfc18a5
  - Grafted path B to A
  
  
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
  $ hg graft -qr $B --from-path A --to-path C
  $ hg show
  commit:      de54b74c0bb1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       C
  description:
  B
  
  Grafted from eb8f2e58912725da3773edc0e24d884469f2bb1c
  - Grafted path A to C
  
  
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
  $ hg graft -qr $D --from-path dir --to-path dir2 --from-path dir --to-path dir3
  $ hg show
  commit:      cea65f1f0bd8
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dir2/file dir3/file
  description:
  D
  
  Grafted from 08771e12ccbd5547592676e9a972caafcd7b0820
  - Grafted path dir to dir2
  - Grafted path dir to dir3
  
  
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
  $ hg graft -qr $G --from-path dir --to-path dir2 --from-path dir --to-path dir3
  $ hg show
  commit:      b4dc750ff25c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dir2/rename2 dir3/rename3
  description:
  G
  
  Grafted from fab9c6fdbcd0fc0139ace494073efb5c40011ed1
  - Grafted path dir to dir2
  - Grafted path dir to dir3
  
  
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
  $ hg graft -qr $F --from-path dir --to-path dir2
  $ hg show
  commit:      db713b5959de
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dir2/rename2
  description:
  F
  
  Grafted from dacfc2aa45adb71c3c557083202bd9178b2e7485
  - Grafted path dir to dir2
  
  
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
  $ hg graft -qr $E --from-path dir4 --to-path dir3
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
  $ hg graft -qr $B --from-path foo --to-path bar
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
  $ hg graft --continue
  grafting dfb58fd2ac21 "B"
  $ hg show
  commit:      2d9de56e5111
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file
  description:
  B
  
  
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
  $ hg graft -qr $B --from-path foo --to-path bar
  local [local] changed bar/file which other [graft] deleted
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
  $ hg graft --continue
  grafting a088319ec9f4 "B"
  $ hg show
  commit:      5ce674a6db6a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file
  description:
  B
  
  
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
  $ hg graft -qr $B --from-path B --to-path A --no-log
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
  $ hg graft -qr 'desc("Summary")' --from-path B --to-path A --config extensions.fbcodereview=
  $ hg show
  commit:      6028184b9e0b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A
  description:
  B
  
  Summary:
  
  Foo
  
  Grafted from e8470334d2058106534ac7d72485e6bfaa76ca01
  - Grafted path B to A
  
  
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
  $ hg graft -qr 'desc("Differential")' --from-path B --to-path A --config extensions.fbcodereview=
  $ hg show
  commit:      0392e4e5a893
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A
  description:
  B
  
  Summary:
  
  Foo
  
  Grafted from 6a2f4620ac267db57400b129af01ac66f3cf2311 (D123)
  - Grafted path B to A
  
  
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
  $ hg graft -qr $C --from-path foo --to-path bar
  $ hg show
  commit:      3e70c43a4deb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       my/bar/file
  description:
  C
  
  Grafted from 48b96237613e0f4a5fb16198b55dd4a03ca3c527
  - Grafted path foo to bar
  
  
  diff --git a/my/bar/file b/my/bar/file
  --- a/my/bar/file
  +++ b/my/bar/file
  @@ -1,3 +1,3 @@
  -a
  +aa
   b
   cc
