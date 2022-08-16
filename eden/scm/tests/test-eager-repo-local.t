#debugruntest-compatible
#chg-compatible

  $ configure modern
  $ setconfig format.use-eager-repo=True

  $ newrepo e1
  $ drawdag << 'EOS'
  > E  # bookmark master = E
  > |
  > D
  > |
  > C  # bookmark stable = C
  > |
  > B
  > |
  > A
  > EOS

Read from the repo

  $ hg log -pr $E
  commit:      9bc730a19041
  bookmark:    master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     E
  
  diff -r f585351a92f8 -r 9bc730a19041 E
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/E	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +E
  \ No newline at end of file

  $ hg bookmarks
     master                    9bc730a19041
     stable                    26805aba1e60

Bookmarks

  $ hg book -d stable
  $ hg book stable -r $B
  $ hg bookmarks
     master                    9bc730a19041
     stable                    112478962961

Rename

  $ hg up -q $E
  $ hg mv E E1
  $ hg st
  A E1
  R E
  $ hg ci -m E1

XXX: rename is tracked but not shown
  $ hg log -p -r .
  commit:      bb41b36a84b5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     E1
  
  diff -r 9bc730a19041 -r bb41b36a84b5 E
  --- a/E	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -E
  \ No newline at end of file
  diff -r 9bc730a19041 -r bb41b36a84b5 E1
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/E1	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +E
  \ No newline at end of file

Export to revlog repo:
  $ hg debugexportrevlog "$TESTTMP/export-revlog"
