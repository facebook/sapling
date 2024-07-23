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
