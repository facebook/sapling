Test that rename detection works
  $ . "$TESTDIR/testutil"

  $ cat >> $HGRCPATH <<EOF
  > [diff]
  > git = True
  > [git]
  > similarity = 50
  > EOF

  $ git init -q gitrepo
  $ cd gitrepo
  $ for i in $(seq 1 10); do echo $i >> alpha; done
  $ git add alpha
  $ fn_git_commit -malpha

Rename a file
  $ git mv alpha beta
  $ echo 11 >> beta
  $ git add beta
  $ fn_git_commit -mbeta

Copy a file
  $ cp beta gamma
  $ echo 12 >> beta
  $ echo 13 >> gamma
  $ git add beta gamma
  $ fn_git_commit -mgamma

Add a submodule (gitlink) and move it to a different spot:
  $ cd ..
  $ git init -q gitsubmodule
  $ cd gitsubmodule
  $ touch subalpha
  $ git add subalpha
  $ fn_git_commit -msubalpha
  $ cd ../gitrepo

  $ rmpwd="import sys; print sys.stdin.read().replace('$(dirname $(pwd))/', '')"
  $ clonefilt='s/Cloning into/Initialized empty Git repository in/;s/in .*/in .../'

  $ git submodule add ../gitsubmodule 2>&1 | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
  Initialized empty Git repository in ...
  
  $ fn_git_commit -m 'add submodule'
  $ sed -e 's/path = gitsubmodule/path = gitsubmodule2/' .gitmodules > .gitmodules-new
  $ mv .gitmodules-new .gitmodules
  $ mv gitsubmodule gitsubmodule2
  $ git add .gitmodules gitsubmodule2
  $ git rm --cached gitsubmodule
  rm 'gitsubmodule'
  $ fn_git_commit -m 'move submodule'

Rename a file elsewhere and replace it with a symlink:

  $ git mv beta beta-new
  $ ln -s beta-new beta
  $ git add beta
  $ fn_git_commit -m 'beta renamed'

Rename the file back:

  $ git rm beta
  rm 'beta'
  $ git mv beta-new beta
  $ fn_git_commit -m 'beta renamed back'

  $ git checkout -f -b not-master 2>&1 | sed s/\'/\"/g
  Switched to a new branch "not-master"

  $ cd ..
  $ hg clone -q gitrepo hgrepo
  $ cd hgrepo
  $ hg log -p --graph --template "{rev} {node} {desc|firstline}\n{join(extras, ' ')}\n\n"
  @  6 10614bb16f4d240ba81b6a71d76a7aa160621a29 beta renamed back
  |  branch=default hg-git-rename-source=git
  |
  |  diff --git a/beta b/beta
  |  old mode 120000
  |  new mode 100644
  |  --- a/beta
  |  +++ b/beta
  |  @@ -1,1 +1,12 @@
  |  -beta-new
  |  \ No newline at end of file
  |  +1
  |  +2
  |  +3
  |  +4
  |  +5
  |  +6
  |  +7
  |  +8
  |  +9
  |  +10
  |  +11
  |  +12
  |  diff --git a/beta-new b/beta-new
  |  deleted file mode 100644
  |  --- a/beta-new
  |  +++ /dev/null
  |  @@ -1,12 +0,0 @@
  |  -1
  |  -2
  |  -3
  |  -4
  |  -5
  |  -6
  |  -7
  |  -8
  |  -9
  |  -10
  |  -11
  |  -12
  |
  o  5 96ad24db491a180ccd330556129d75377e201f63 beta renamed
  |  branch=default hg-git-rename-source=git
  |
  |  diff --git a/beta b/beta
  |  old mode 100644
  |  new mode 120000
  |  --- a/beta
  |  +++ b/beta
  |  @@ -1,12 +1,1 @@
  |  -1
  |  -2
  |  -3
  |  -4
  |  -5
  |  -6
  |  -7
  |  -8
  |  -9
  |  -10
  |  -11
  |  -12
  |  +beta-new
  |  \ No newline at end of file
  |  diff --git a/beta b/beta-new
  |  copy from beta
  |  copy to beta-new
  |
  o  4 d22608e850ea875936802e119831f1789f5d98bd move submodule
  |  branch=default hg-git-rename-source=git
  |
  |  diff --git a/.gitmodules b/.gitmodules
  |  --- a/.gitmodules
  |  +++ b/.gitmodules
  |  @@ -1,3 +1,3 @@
  |   [submodule "gitsubmodule"]
  |  -	path = gitsubmodule
  |  +	path = gitsubmodule2
  |   	url = ../gitsubmodule
  |  diff --git a/.hgsub b/.hgsub
  |  --- a/.hgsub
  |  +++ b/.hgsub
  |  @@ -1,1 +1,1 @@
  |  -gitsubmodule = [git]../gitsubmodule
  |  +gitsubmodule2 = [git]../gitsubmodule
  |  diff --git a/.hgsubstate b/.hgsubstate
  |  --- a/.hgsubstate
  |  +++ b/.hgsubstate
  |  @@ -1,1 +1,1 @@
  |  -5944b31ff85b415573d1a43eb942e2dea30ab8be gitsubmodule
  |  +5944b31ff85b415573d1a43eb942e2dea30ab8be gitsubmodule2
  |
  o  3 db55fd0e7083555ec886f6175fa0a42a711c6592 add submodule
  |  branch=default hg-git-rename-source=git
  |
  |  diff --git a/.gitmodules b/.gitmodules
  |  new file mode 100644
  |  --- /dev/null
  |  +++ b/.gitmodules
  |  @@ -0,0 +1,3 @@
  |  +[submodule "gitsubmodule"]
  |  +	path = gitsubmodule
  |  +	url = ../gitsubmodule
  |  diff --git a/.hgsub b/.hgsub
  |  new file mode 100644
  |  --- /dev/null
  |  +++ b/.hgsub
  |  @@ -0,0 +1,1 @@
  |  +gitsubmodule = [git]../gitsubmodule
  |  diff --git a/.hgsubstate b/.hgsubstate
  |  new file mode 100644
  |  --- /dev/null
  |  +++ b/.hgsubstate
  |  @@ -0,0 +1,1 @@
  |  +5944b31ff85b415573d1a43eb942e2dea30ab8be gitsubmodule
  |
  o  2 20f9e56b6d006d0403f853245e483d0892b8ac48 gamma
  |  branch=default hg-git-rename-source=git
  |
  |  diff --git a/beta b/beta
  |  --- a/beta
  |  +++ b/beta
  |  @@ -9,3 +9,4 @@
  |   9
  |   10
  |   11
  |  +12
  |  diff --git a/beta b/gamma
  |  copy from beta
  |  copy to gamma
  |  --- a/beta
  |  +++ b/gamma
  |  @@ -9,3 +9,4 @@
  |   9
  |   10
  |   11
  |  +13
  |
  o  1 9f7744e68def81da3b394f11352f602ca9c8ab68 beta
  |  branch=default hg-git-rename-source=git
  |
  |  diff --git a/alpha b/beta
  |  rename from alpha
  |  rename to beta
  |  --- a/alpha
  |  +++ b/beta
  |  @@ -8,3 +8,4 @@
  |   8
  |   9
  |   10
  |  +11
  |
  o  0 7bc844166f76e49562f81eacd54ea954d01a9e42 alpha
     branch=default hg-git-rename-source=git
  
     diff --git a/alpha b/alpha
     new file mode 100644
     --- /dev/null
     +++ b/alpha
     @@ -0,0 +1,10 @@
     +1
     +2
     +3
     +4
     +5
     +6
     +7
     +8
     +9
     +10
  

Make a new ordinary commit in Mercurial (no extra metadata)
  $ echo 14 >> gamma
  $ hg ci -m "gamma2"

Make a new commit with a copy and a rename in Mercurial
  $ hg cp gamma delta
  $ echo 15 >> delta
  $ hg mv beta epsilon
  $ echo 16 >> epsilon
  $ hg ci -m "delta/epsilon"
  $ hg export .
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID fc770f3d5429f9406cb45eaf4331e16d5f7a700d
  # Parent  8542264382fc0ad8acf981974805d73bf89e9521
  delta/epsilon
  
  diff --git a/gamma b/delta
  copy from gamma
  copy to delta
  --- a/gamma
  +++ b/delta
  @@ -11,3 +11,4 @@
   11
   13
   14
  +15
  diff --git a/beta b/epsilon
  rename from beta
  rename to epsilon
  --- a/beta
  +++ b/epsilon
  @@ -10,3 +10,4 @@
   10
   11
   12
  +16
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 2 commits with 2 trees and 3 blobs
  updating reference refs/heads/master

  $ cd ../gitrepo
  $ git log master --pretty=oneline
  254e71fefd695af5bcbac61c6e5e57cbbada37b8 delta/epsilon
  bf71b4d53de0f136931cc80482b1d1f47162630a gamma2
  f95497455dfa891b4cd9b524007eb9514c3ab654 beta renamed back
  055f482277da6cd3dd37c7093d06983bad68f782 beta renamed
  d7f31298f27df8a9226eddb1e4feb96922c46fa5 move submodule
  c610256cb6959852d9e70d01902a06726317affc add submodule
  e1348449e0c3a417b086ed60fc13f068d4aa8b26 gamma
  cc83241f39927232f690d370894960b0d1943a0e beta
  938bb65bb322eb4a3558bec4cdc8a680c4d1794c alpha

Make sure the right metadata is stored
  $ git cat-file commit master^
  tree 0adbde18545845f3b42ad1a18939ed60a9dec7a8
  parent f95497455dfa891b4cd9b524007eb9514c3ab654
  author test <none@none> 0 +0000
  committer test <none@none> 0 +0000
  HG:rename-source hg
  
  gamma2
  $ git cat-file commit master
  tree f8f32f4e20b56a5a74582c6a5952c175bf9ec155
  parent bf71b4d53de0f136931cc80482b1d1f47162630a
  author test <none@none> 0 +0000
  committer test <none@none> 0 +0000
  HG:rename gamma:delta
  HG:rename beta:epsilon
  
  delta/epsilon

Now make another clone and compare the hashes

  $ cd ..
  $ hg clone -q gitrepo hgrepo2
  $ cd hgrepo2
  $ hg export master
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID fc770f3d5429f9406cb45eaf4331e16d5f7a700d
  # Parent  8542264382fc0ad8acf981974805d73bf89e9521
  delta/epsilon
  
  diff --git a/gamma b/delta
  copy from gamma
  copy to delta
  --- a/gamma
  +++ b/delta
  @@ -11,3 +11,4 @@
   11
   13
   14
  +15
  diff --git a/beta b/epsilon
  rename from beta
  rename to epsilon
  --- a/beta
  +++ b/epsilon
  @@ -10,3 +10,4 @@
   10
   11
   12
  +16

Regenerate the Git metadata and compare the hashes
  $ hg gclear
  clearing out the git cache data
  $ hg gexport
  $ cd .hg/git
  $ git log master --pretty=oneline
  254e71fefd695af5bcbac61c6e5e57cbbada37b8 delta/epsilon
  bf71b4d53de0f136931cc80482b1d1f47162630a gamma2
  f95497455dfa891b4cd9b524007eb9514c3ab654 beta renamed back
  055f482277da6cd3dd37c7093d06983bad68f782 beta renamed
  d7f31298f27df8a9226eddb1e4feb96922c46fa5 move submodule
  c610256cb6959852d9e70d01902a06726317affc add submodule
  e1348449e0c3a417b086ed60fc13f068d4aa8b26 gamma
  cc83241f39927232f690d370894960b0d1943a0e beta
  938bb65bb322eb4a3558bec4cdc8a680c4d1794c alpha
