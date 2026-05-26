
#require no-eden


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

  $ sl log -pr $E
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

  $ sl bookmarks
     master                    9bc730a19041
     stable                    26805aba1e60

Bookmarks

  $ sl book -d stable
  $ sl book stable -r $B
  $ sl bookmarks
     master                    9bc730a19041
     stable                    112478962961

Rename

  $ sl up -q $E
  $ sl mv E E1
  $ sl st
  A E1
  R E
  $ sl ci -m E1

  $ sl log -p -r . --config diff.git=true
  commit:      bb41b36a84b5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     E1
  
  diff --git a/E b/E1
  rename from E
  rename to E1
