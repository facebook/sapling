  $ hg init

  $ python -c 'file("a", "wb").write("confuse str.splitlines\nembedded\rnewline\n")'
  $ hg ci -Ama -d '1 0'
  adding a

  $ echo clean diff >> a
  $ hg ci -mb -d '2 0'

  $ hg diff -r0 -r1
  diff -r 107ba6f817b5 -r 310ce7989cdc a
  --- a/a	Thu Jan 01 00:00:01 1970 +0000
  +++ b/a	Thu Jan 01 00:00:02 1970 +0000
  @@ -1,2 +1,3 @@
   confuse str.splitlines
   embedded\r (no-eol) (esc)
  newline
  +clean diff

