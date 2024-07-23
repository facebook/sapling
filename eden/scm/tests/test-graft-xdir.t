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

  $ newclientrepo
  $ drawdag <<EOS
  > C B  # B/bar/file = a\nb\ncc\n (copied from foo/file)
  > |/   # C/foo/file = aa\nb\nc\n
  > A    # A/foo/file = a\nb\nc\n
  > EOS
  $ hg go -q $B
  $ hg graft -qr $C --from-path foo --to-path bar
  abort: bar/file@439eb343cae6: not found in manifest!
  [255]
FIXME: we want to graft to bar/file, not foo/file
  $ hg show
  commit:      7599131684a9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       B bar/file
  description:
  B
  
  
  diff --git a/B b/B
  new file mode 100644
  --- /dev/null
  +++ b/B
  @@ -0,0 +1,1 @@
  +B
  \ No newline at end of file
  diff --git a/foo/file b/bar/file
  copy from foo/file
  copy to bar/file
  --- a/foo/file
  +++ b/bar/file
  @@ -1,3 +1,3 @@
   a
   b
  -c
  +cc
