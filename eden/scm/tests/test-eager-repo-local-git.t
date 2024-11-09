#require no-eden

  $ configure modern
  $ setconfig format.use-eager-repo=True

  $ newrepo e1-git
  $ grep 'git|eager' .hg/store/requires
  eagerepo
  git
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
  commit:      aca920ced755
  bookmark:    master
  user:        test <>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     E
  
  diff -r 149951656031 -r aca920ced755 E
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/E	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +E
  \ No newline at end of file

  $ hg bookmarks
     master                    aca920ced755
     stable                    06625e541e53

Bookmarks

  $ hg book -d stable
  $ hg book stable -r $B
  $ hg bookmarks
     master                    aca920ced755
     stable                    0de30934572f

Rename

  $ hg up -q $E
  $ hg mv E E1
  $ hg st
  A E1
  R E
  $ hg ci -m E1

  $ hg log -p -r . --config diff.git=true
  commit:      de9436c587d7
  user:        test <>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     E1
  
  diff --git a/E b/E1
  rename from E
  rename to E1

