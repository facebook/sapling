  $ setconfig diff.git=true

  $ newclientrepo
  $ drawdag <<EOS
  > C B  # B/bar/file = a\nb\ncc\n (copied from foo/file)
  > |/   # C/foo/file = aa\nb\nc\n
  > A    # A/foo/file = a\nb\nc\n
  > EOS
  $ hg go -q $B
  $ hg graft -qr $C --from-path foo --to-path bar
FIXME: we want to graft to bar/file, not foo/file
  $ hg show
  commit:      55191f3cddb4
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       C foo/file
  description:
  C
  
  
  diff --git a/C b/C
  new file mode 100644
  --- /dev/null
  +++ b/C
  @@ -0,0 +1,1 @@
  +C
  \ No newline at end of file
  diff --git a/foo/file b/foo/file
  --- a/foo/file
  +++ b/foo/file
  @@ -1,3 +1,3 @@
  -a
  +aa
   b
   c
