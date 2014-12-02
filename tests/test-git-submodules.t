Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ git init gitrepo1
  Initialized empty Git repository in $TESTTMP/gitrepo1/.git/
  $ cd gitrepo1
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'
  $ cd ..

  $ git init gitsubrepo
  Initialized empty Git repository in $TESTTMP/gitsubrepo/.git/
  $ cd gitsubrepo
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'
  $ cd ..

  $ mkdir gitrepo2
  $ cd gitrepo2

  $ rmpwd="import sys; print sys.stdin.read().replace('$(dirname $(pwd))/', '')"
different versions of git spell the dir differently. Older versions
use the full path to the directory all the time, whereas newer
version spell it sanely as it was given (eg . in a newer version,
while older git will use the full normalized path for .)
  $ clonefilt='s/Cloning into/Initialized empty Git repository in/;s/in .*/in .../'

  $ git clone ../gitrepo1 . 2>&1 | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
  Initialized empty Git repository in ...
  
  $ git submodule add ../gitsubrepo subrepo 2>&1 | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
  Initialized empty Git repository in ...
  
  $ git commit -m 'add subrepo' | sed 's/, 0 deletions(-)//'
  [master e42b08b] add subrepo
   2 files changed, 4 insertions(+)
   create mode 100644 .gitmodules
   create mode 160000 subrepo
  $ cd subrepo
  $ echo gamma > gamma
  $ git add gamma
  $ fn_git_commit -m 'add gamma'
  $ cd ..
  $ git add subrepo
  $ git commit -m 'change subrepo commit'
  [master a000567] change subrepo commit
   1 file changed, 1 insertion(+), 1 deletion(-)

  $ git submodule add ../gitsubrepo subrepo2 2>&1 | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
  Initialized empty Git repository in ...
  
  $ git commit -m 'add another subrepo' | sed 's/, 0 deletions(-)//'
  [master 6e21952] add another subrepo
   2 files changed, 4 insertions(+)
   create mode 160000 subrepo2

remove one subrepo, replace with file

  $ git rm --cached subrepo
  rm 'subrepo'
we'd ordinarily use sed here, but BSD sed doesn't support two-address formats
like +2 -- so use grep with the stuff we want to keep
  $ grep 'submodule "subrepo2"' -A2 .gitmodules > .gitmodules-new
  $ mv .gitmodules-new .gitmodules
  $ git add .gitmodules
  $ git config --unset submodule.subrepo.url
  $ rm -rf subrepo
  $ echo subrepo > subrepo
  $ git add subrepo
  $ git commit -m 'replace subrepo with file' | sed 's/, 0 deletions(-)//' | sed 's/, 0 insertions(+)//'
  [master f6436a4] replace subrepo with file
   2 files changed, 1 insertion(+), 4 deletions(-)
   mode change 160000 => 100644 subrepo

replace file with subrepo -- apparently, git complains about the subrepo if the
same name has existed at any point historically, so use alpha instead of subrepo

  $ git rm alpha
  rm 'alpha'
  $ git submodule add ../gitsubrepo alpha 2>&1 | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
  Initialized empty Git repository in ...
  
  $ git commit -m 'replace file with subrepo' | sed 's/, 0 deletions(-)//' | sed 's/, 0 insertions(+)//'
  [master 8817116] replace file with subrepo
   2 files changed, 4 insertions(+), 1 deletion(-)
   mode change 100644 => 160000 alpha

  $ git rm --cached subrepo2
  rm 'subrepo2'
  $ git rm --cached alpha
  rm 'alpha'
  $ git rm .gitmodules
  rm '.gitmodules'
  $ git commit -m 'remove all subrepos' | sed 's/, 0 deletions(-)//' | sed 's/, 0 insertions(+)//'
  [master d3c4728] remove all subrepos
   3 files changed, 8 deletions(-)
   delete mode 100644 .gitmodules
   delete mode 160000 alpha
   delete mode 160000 subrepo2

  $ git log --pretty=oneline
  d3c472800f9d11baa6615971a3179fd441869173 remove all subrepos
  88171163bf4795b5570924e51d5f8ede33f8bc28 replace file with subrepo
  f6436a472da00f581d8d257e9bbaf3c358a5e88c replace subrepo with file
  6e219527869fa40eb6ffbdd013cd86d576b26b01 add another subrepo
  a000567ceefbd9a2ce364e0dea6e298010b02b6d change subrepo commit
  e42b08b3cb7069b4594a4ee1d9cb641ee47b2355 add subrepo
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 add alpha

  $ cd ..

  $ hg clone gitrepo2 hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log --graph  | grep -v ': *master'
  @  changeset:   6:b52cbe4c3ffe
  |  tag:         default/master
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     remove all subrepos
  |
  o  changeset:   5:87bae50d72cb
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace file with subrepo
  |
  o  changeset:   4:33729ae46d57
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace subrepo with file
  |
  o  changeset:   3:4d2f0f4fb53d
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add another subrepo
  |
  o  changeset:   2:620c9d5e9a98
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     change subrepo commit
  |
  o  changeset:   1:f20b40ad6da1
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add subrepo
  |
  o  changeset:   0:ff7a2f2d8d70
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
  $ hg book
   * master                    6:b52cbe4c3ffe

(add subrepo)
  $ hg cat -r 1 .hgsubstate
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo
  $ hg cat -r 1 .hgsub
  subrepo = [git]../gitsubrepo
  $ hg gverify -r 1
  verifying rev f20b40ad6da1 against git commit e42b08b3cb7069b4594a4ee1d9cb641ee47b2355

(change subrepo commit)
  $ hg cat -r 2 .hgsubstate
  aa2ead20c29b5cc6256408e1d9ef704870033afb subrepo
  $ hg cat -r 2 .hgsub
  subrepo = [git]../gitsubrepo
  $ hg gverify -r 2
  verifying rev 620c9d5e9a98 against git commit a000567ceefbd9a2ce364e0dea6e298010b02b6d

(add another subrepo)
  $ hg cat -r 3 .hgsubstate
  aa2ead20c29b5cc6256408e1d9ef704870033afb subrepo
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo2
  $ hg cat -r 3 .hgsub
  subrepo = [git]../gitsubrepo
  subrepo2 = [git]../gitsubrepo
  $ hg gverify -r 3
  verifying rev 4d2f0f4fb53d against git commit 6e219527869fa40eb6ffbdd013cd86d576b26b01

(replace subrepo with file)
  $ hg cat -r 4 .hgsubstate
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo2
  $ hg cat -r 4 .hgsub
  subrepo2 = [git]../gitsubrepo
  $ hg manifest -r 4
  .gitmodules
  .hgsub
  .hgsubstate
  alpha
  subrepo
  $ hg gverify -r 4
  verifying rev 33729ae46d57 against git commit f6436a472da00f581d8d257e9bbaf3c358a5e88c

(replace file with subrepo)
  $ hg cat -r 5 .hgsubstate
  6e4ad8da50204560c00fa25e4987eb2e239029ba alpha
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo2
  $ hg cat -r 5 .hgsub
  subrepo2 = [git]../gitsubrepo
  alpha = [git]../gitsubrepo
  $ hg manifest -r 5
  .gitmodules
  .hgsub
  .hgsubstate
  subrepo
  $ hg gverify -r 5
  verifying rev 87bae50d72cb against git commit 88171163bf4795b5570924e51d5f8ede33f8bc28

(remove all subrepos)
  $ hg cat -r 6 .hgsub .hgsubstate
  .hgsub: no such file in rev b52cbe4c3ffe
  .hgsubstate: no such file in rev b52cbe4c3ffe
  [1]
  $ hg gverify -r 6
  verifying rev b52cbe4c3ffe against git commit d3c472800f9d11baa6615971a3179fd441869173

  $ hg gclear
  clearing out the git cache data
  $ hg gexport
  $ cd .hg/git
  $ git log --pretty=oneline
  d3c472800f9d11baa6615971a3179fd441869173 remove all subrepos
  88171163bf4795b5570924e51d5f8ede33f8bc28 replace file with subrepo
  f6436a472da00f581d8d257e9bbaf3c358a5e88c replace subrepo with file
  6e219527869fa40eb6ffbdd013cd86d576b26b01 add another subrepo
  a000567ceefbd9a2ce364e0dea6e298010b02b6d change subrepo commit
  e42b08b3cb7069b4594a4ee1d9cb641ee47b2355 add subrepo
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 add alpha
