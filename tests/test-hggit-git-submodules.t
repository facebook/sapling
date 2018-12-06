Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

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

  $ git clone ../gitrepo1 .
  Cloning into '.'...
  done.

  $ git submodule add ../gitsubrepo subrepo
  Cloning into '$TESTTMP/gitrepo2/subrepo'...
  done.

  $ git commit -m 'add subrepo'
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

  $ git submodule add ../gitsubrepo subrepo2
  Cloning into '$TESTTMP/gitrepo2/subrepo2'...
  done.

  $ git commit -m 'add another subrepo'
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
  $ git commit -m 'replace subrepo with file'
  [master f6436a4] replace subrepo with file
   2 files changed, 1 insertion(+), 4 deletions(-)
   mode change 160000 => 100644 subrepo

replace file with subrepo -- apparently, git complains about the subrepo if the
same name has existed at any point historically, so use alpha instead of subrepo

  $ git rm alpha
  rm 'alpha'
  $ git submodule add ../gitsubrepo alpha
  Cloning into '$TESTTMP/gitrepo2/alpha'...
  done.
  $ git commit -m 'replace file with subrepo'
  [master 8817116] replace file with subrepo
   2 files changed, 4 insertions(+), 1 deletion(-)
   mode change 100644 => 160000 alpha

  $ ln -s foo foolink
  $ git add foolink
  $ git commit -m 'add symlink'
  [master 2d1c135] add symlink
   1 file changed, 1 insertion(+)
   create mode 120000 foolink

replace symlink with subrepo

  $ git rm foolink
  rm 'foolink'
  $ git submodule add ../gitsubrepo foolink
  Cloning into '$TESTTMP/gitrepo2/foolink'...
  done.
  $ git commit -m 'replace symlink with subrepo'
  [master e3288fa] replace symlink with subrepo
   2 files changed, 4 insertions(+), 1 deletion(-)
   mode change 120000 => 160000 foolink

replace subrepo with symlink

  $ cat > .gitmodules <<EOF
  > [submodule "subrepo2"]
  > 	path = subrepo2
  > 	url = ../gitsubrepo
  > [submodule "alpha"]
  > 	path = alpha
  > 	url = ../gitsubrepo
  > EOF
  $ git add .gitmodules
  $ git rm --cached foolink
  rm 'foolink'
  $ rm -rf foolink
  $ ln -s foo foolink
  $ git add foolink
  $ git commit -m 'replace subrepo with symlink'
  [master d283640] replace subrepo with symlink
   2 files changed, 1 insertion(+), 4 deletions(-)
   mode change 160000 => 120000 foolink
  $ git show
  commit d28364013fe1a0fde56c0e1921e49ecdeee8571d
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      replace subrepo with symlink
  
  diff --git a/.gitmodules b/.gitmodules
  index b511494..813e20b 100644
  --- a/.gitmodules
  +++ b/.gitmodules
  @@ -4,6 +4,3 @@
   [submodule "alpha"]
   	path = alpha
   	url = ../gitsubrepo
  -[submodule "foolink"]
  -	path = foolink
  -	url = ../gitsubrepo
  diff --git a/foolink b/foolink
  deleted file mode 160000
  index 6e4ad8d..0000000
  --- a/foolink
  +++ /dev/null
  @@ -1* +0,0 @@ (glob)
  -Subproject commit 6e4ad8da50204560c00fa25e4987eb2e239029ba
  diff --git a/foolink b/foolink
  new file mode 120000
  index 0000000..1910281
  --- /dev/null
  +++ b/foolink
  @@ -0,0 +1* @@ (glob)
  +foo
  \ No newline at end of file

  $ git rm --cached subrepo2
  rm 'subrepo2'
  $ git rm --cached alpha
  rm 'alpha'
  $ git rm .gitmodules
  rm '.gitmodules'
  $ git commit -m 'remove all subrepos'
  [master 15ba949] remove all subrepos
   3 files changed, 8 deletions(-)
   delete mode 100644 .gitmodules
   delete mode 160000 alpha
   delete mode 160000 subrepo2

  $ git log --pretty=oneline
  15ba94929481c654814178aac1dbca06ae688718 remove all subrepos
  d28364013fe1a0fde56c0e1921e49ecdeee8571d replace subrepo with symlink
  e3288fa737d429a60637b3b6782cb25b8298bc00 replace symlink with subrepo
  2d1c135447d11df4dfe96dd5d4f37926dc5c821d add symlink
  88171163bf4795b5570924e51d5f8ede33f8bc28 replace file with subrepo
  f6436a472da00f581d8d257e9bbaf3c358a5e88c replace subrepo with file
  6e219527869fa40eb6ffbdd013cd86d576b26b01 add another subrepo
  a000567ceefbd9a2ce364e0dea6e298010b02b6d change subrepo commit
  e42b08b3cb7069b4594a4ee1d9cb641ee47b2355 add subrepo
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 add alpha

  $ cd ..

  $ hg clone gitrepo2 hgrepo | grep -v '^updating'
  importing git objects into hg
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log --graph
  @  changeset:   9:5ae8371d90fe
  |  bookmark:    master
  |  tag:         default/master
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     remove all subrepos
  |
  o  changeset:   8:3d35b3b681ad
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace subrepo with symlink
  |
  o  changeset:   7:7ab2f3f0d2a2
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace symlink with subrepo
  |
  o  changeset:   6:10077550ca45
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add symlink
  |
  o  changeset:   5:5ccecec21679
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace file with subrepo
  |
  o  changeset:   4:a44b8fb5038d
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace subrepo with file
  |
  o  changeset:   3:fa3b1061c069
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add another subrepo
  |
  o  changeset:   2:61810bd16e46
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     change subrepo commit
  |
  o  changeset:   1:ab274969926a
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add subrepo
  |
  o  changeset:   0:69982ec78c6d
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
  $ hg book
   * master                    9:5ae8371d90fe

(add subrepo)
  $ hg cat -r 1 .hgsubstate
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo
  $ hg cat -r 1 .hgsub
  subrepo = [git]../gitsubrepo
  $ hg gverify -r 1
  verifying rev ab274969926a against git commit e42b08b3cb7069b4594a4ee1d9cb641ee47b2355

(change subrepo commit)
  $ hg cat -r 2 .hgsubstate
  aa2ead20c29b5cc6256408e1d9ef704870033afb subrepo
  $ hg cat -r 2 .hgsub
  subrepo = [git]../gitsubrepo
  $ hg gverify -r 2
  verifying rev 61810bd16e46 against git commit a000567ceefbd9a2ce364e0dea6e298010b02b6d

(add another subrepo)
  $ hg cat -r 3 .hgsubstate
  aa2ead20c29b5cc6256408e1d9ef704870033afb subrepo
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo2
  $ hg cat -r 3 .hgsub
  subrepo = [git]../gitsubrepo
  subrepo2 = [git]../gitsubrepo
  $ hg gverify -r 3
  verifying rev fa3b1061c069 against git commit 6e219527869fa40eb6ffbdd013cd86d576b26b01

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
  verifying rev a44b8fb5038d against git commit f6436a472da00f581d8d257e9bbaf3c358a5e88c

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
  verifying rev 5ccecec21679 against git commit 88171163bf4795b5570924e51d5f8ede33f8bc28

(replace symlink with subrepo)
XXX: The new logic in core is too strict but we don't really care about this usecase so
we just ignore this failure for now.
  $ hg cat -r 7 .hgsub .hgsubstate
  subrepo2 = [git]../gitsubrepo
  alpha = [git]../gitsubrepo
  foolink = [git]../gitsubrepo
  6e4ad8da50204560c00fa25e4987eb2e239029ba alpha
  6e4ad8da50204560c00fa25e4987eb2e239029ba foolink
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo2
  abort: subrepo 'foolink' traverses symbolic link
  [255]
  $ hg gverify -r 7
  verifying rev 7ab2f3f0d2a2 against git commit e3288fa737d429a60637b3b6782cb25b8298bc00

(replace subrepo with symlink)
  $ hg cat -r 8 .hgsub .hgsubstate
  subrepo2 = [git]../gitsubrepo
  alpha = [git]../gitsubrepo
  6e4ad8da50204560c00fa25e4987eb2e239029ba alpha
  6e4ad8da50204560c00fa25e4987eb2e239029ba subrepo2

  $ hg gverify -r 8
  verifying rev 3d35b3b681ad against git commit d28364013fe1a0fde56c0e1921e49ecdeee8571d

(remove all subrepos)
  $ hg cat -r 9 .hgsub .hgsubstate
  .hgsub: no such file in rev 5ae8371d90fe
  .hgsubstate: no such file in rev 5ae8371d90fe
  [1]
  $ hg gverify -r 9
  verifying rev 5ae8371d90fe against git commit 15ba94929481c654814178aac1dbca06ae688718

  $ hg gclear
  clearing out the git cache data
  $ hg gexport
  $ cd .hg/git
  $ git log --pretty=oneline
  5029f164081f610d376405968d4588b823810838 remove all subrepos
  80ac0dcee3a4f86fdb7bab740f737f2cd4b19182 replace subrepo with symlink
  9c650056de9a0a417e5590a588bf4e942d378519 replace symlink with subrepo
  0d13ab8294c9c35f5af94dc8af2ffc7f96fb395b add symlink
  d3ce4262b9bc8e1f7f6497b8039627f073b77426 replace file with subrepo
  71941511905ee6178d184519ff131468c2f84241 replace subrepo with file
  83d542a647b4f08344e3937697efb936dcb1d178 add another subrepo
  6e6d32168939af1a292dc85b5f737c95dbde349c change subrepo commit
  14951d27ab1a586eede27e0b10e8d29f5b070743 add subrepo
  205598a42833e532ad20d80414b8e3b85a65936e add alpha

test with rename detection enabled -- simply checking that the Mercurial hashes
are the same is enough
  $ cd ../../..
  $ hg --config git.similarity=100 clone gitrepo2 hgreporenames | grep -v '^updating'
  importing git objects into hg
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgreporenames
  $ hg log --graph
  @  changeset:   9:5ae8371d90fe
  |  bookmark:    master
  |  tag:         default/master
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     remove all subrepos
  |
  o  changeset:   8:3d35b3b681ad
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace subrepo with symlink
  |
  o  changeset:   7:7ab2f3f0d2a2
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace symlink with subrepo
  |
  o  changeset:   6:10077550ca45
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add symlink
  |
  o  changeset:   5:5ccecec21679
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace file with subrepo
  |
  o  changeset:   4:a44b8fb5038d
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     replace subrepo with file
  |
  o  changeset:   3:fa3b1061c069
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add another subrepo
  |
  o  changeset:   2:61810bd16e46
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     change subrepo commit
  |
  o  changeset:   1:ab274969926a
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add subrepo
  |
  o  changeset:   0:69982ec78c6d
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
