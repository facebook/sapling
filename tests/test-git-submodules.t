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

  $ git clone ../gitrepo1 . | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
  Initialized empty Git repository in ...
  
  $ git submodule add ../gitsubrepo subrepo | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
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

  $ git submodule add ../gitsubrepo subrepo2 | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
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
  $ git submodule add ../gitsubrepo alpha | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
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

  $ cd ..

  $ hg clone gitrepo2 hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log --graph  | grep -v ': *master'
  @  changeset:   6:827c0345b7d1
  |  tag:         default/master
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     remove all subrepos
  |
  o  changeset:   5:97f89374a0ce
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace file with subrepo
  |
  o  changeset:   4:e233b0858578
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace subrepo with file
  |
  o  changeset:   3:6264517ddb98
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add another subrepo
  |
  o  changeset:   2:914937cccdbe
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     change subrepo commit
  |
  o  changeset:   1:2f69b1b8a6f8
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add subrepo
  |
  o  changeset:   0:3442585be8a6
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
  $ hg book
   * master                    6:827c0345b7d1

(add subrepo)
  $ hg cat -r 1 .hgsubstate
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo
  $ hg cat -r 1 .hgsub
  subrepo = [git]../gitsubrepo

(change subrepo commit)
  $ hg cat -r 2 .hgsubstate
  aa2ead20c29b5cc6256408e1d9ef704870033afb subrepo
  $ hg cat -r 2 .hgsub
  subrepo = [git]../gitsubrepo

(add another subrepo)
  $ hg cat -r 3 .hgsubstate
  aa2ead20c29b5cc6256408e1d9ef704870033afb subrepo
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo2
  $ hg cat -r 3 .hgsub
  subrepo = [git]../gitsubrepo
  subrepo2 = [git]../gitsubrepo

(replace subrepo with file)
  $ hg cat -r 4 .hgsubstate
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo2
  $ hg cat -r 4 .hgsub
  subrepo2 = [git]../gitsubrepo
  $ hg cat -r 4 subrepo
  subrepo

(replace file with subrepo)
  $ hg cat -r 5 .hgsubstate
  6e4ad8da50204560c00fa25e4987eb2e239029ba alpha
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo2
  $ hg cat -r 5 .hgsub
  subrepo2 = [git]../gitsubrepo
  alpha = [git]../gitsubrepo
  $ hg cat -r 5 alpha
  alpha: no such file in rev 97f89374a0ce
  [1]

(remove all subrepos)
  $ hg cat -r 6 .hgsub .hgsubstate
  .hgsub: no such file in rev 827c0345b7d1
  .hgsubstate: no such file in rev 827c0345b7d1
  [1]

