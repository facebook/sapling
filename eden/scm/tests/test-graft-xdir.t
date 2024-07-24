  $ setconfig diff.git=true

Test validation of --from-path and --to-path
  $ newclientrepo
  $ echo "A" | drawdag
  $ hg graft -r $A --from-path foo
  grafting 426bada5c675 "A"
  abort: must provide same number of --from-path and --to-path
  [255]
  $ hg graft -r $A --to-path foo
  grafting 426bada5c675 "A"
  abort: must provide same number of --from-path and --to-path
  [255]
  $ hg graft -r $A --from-path foo --from-path bar --to-path baz --to-path baz/qux
  grafting 426bada5c675 "A"
  abort: overlapping --to-path entries
  [255]
  $ hg graft -r $A --from-path foo --from-path bar --to-path baz --to-path ""
  grafting 426bada5c675 "A"
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
  commit:      c6f2b52276f0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file
  description:
  C
  
  
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
  commit:      1d8d66326bc5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/new
  description:
  C
  
  
  diff --git a/bar/new b/bar/new
  new file mode 100644
  --- /dev/null
  +++ b/bar/new
  @@ -0,0 +1,1 @@
  +new


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
  commit:      4de9783d32fa
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/rename
  description:
  C
  
  
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
  commit:      597c3df28a9e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file bar/rename
  description:
  C
  
  
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
  commit:      54cc7ba455d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file
  description:
  D
  
  
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
  commit:      fa496899ba00
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/rename
  description:
  E
  
  
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
  commit:      424441b2970c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/rename
  description:
  F
  
  
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
  commit:      4b102adaac64
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A
  description:
  C
  
  
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
  commit:      b60c71cdc603
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       C
  description:
  B
  
  
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
  commit:      2995e39b4791
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dir2/file dir3/file
  description:
  D
  
  
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
  commit:      b741cc1c2a84
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dir2/rename2 dir3/rename3
  description:
  G
  
  
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
  commit:      f576590c646e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dir2/rename2
  description:
  F
  
  
  diff --git a/dir2/rename2 b/dir2/rename2
  --- a/dir2/rename2
  +++ b/dir2/rename2
  @@ -1,1 +1,1 @@
  -A
  +AA
