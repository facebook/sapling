Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ git init gitsubrepo
  Initialized empty Git repository in $TESTTMP/gitsubrepo/.git/
  $ cd gitsubrepo
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'
  $ cd ..

  $ git init gitrepo1
  Initialized empty Git repository in $TESTTMP/gitrepo1/.git/
  $ cd gitrepo1
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'
  $ git submodule add ../gitsubrepo subrepo1
  Cloning into 'subrepo1'...
  done.
  $ fn_git_commit -m 'add subrepo1'
  $ git submodule add ../gitsubrepo xyz/subrepo2
  Cloning into 'xyz/subrepo2'...
  done.
  $ fn_git_commit -m 'add subrepo2'
we are going to push to this repo from our hg clone,
allow commits despite working copy presense
  $ git config receive.denyCurrentBranch ignore
  $ cd ..
Ensure gitlinks are transformed to .hgsubstate on hg pull from git
  $ hg clone gitrepo1 hgrepo 2>&1 | egrep -v '^(Cloning into|done)'
  importing git objects into hg
  updating to branch default
  cloning subrepo subrepo1 from $TESTTMP/gitsubrepo
  cloning subrepo xyz/subrepo2 from $TESTTMP/gitsubrepo
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg bookmarks -f -r default master
1. Ensure gitlinks are transformed to .hgsubstate on hg <- git pull
.hgsub shall list two [git] subrepos
  $ cat .hgsub | sort
  subrepo1 = [git]../gitsubrepo
  xyz/subrepo2 = [git]../gitsubrepo
.hgsubstate shall list two idenitcal revisions
  $ cat .hgsubstate
  56f0304c5250308f14cfbafdc27bd12d40154d17 subrepo1
  56f0304c5250308f14cfbafdc27bd12d40154d17 xyz/subrepo2
hg status shall NOT report .hgsub and .hgsubstate as untracked - either ignored or unmodified
  $ hg status --unknown .hgsub .hgsubstate
  $ hg status --modified .hgsub .hgsubstate
  $ cd ..

2. Check gitmodules are preserved during hg -> git push
  $ cd gitsubrepo
  $ echo gamma > gamma
  $ git add gamma
  $ fn_git_commit -m 'add gamma'
  $ cd ..
  $ cd hgrepo
  $ cd xyz/subrepo2
  $ git pull | sed 's/files/file/;s/insertions/insertion/;s/, 0 deletions.*//' | sed 's/|  */| /'
  From $TESTTMP/gitsubrepo
     56f0304..aabf7cd  master     -> origin/master
  Updating 56f0304..aabf7cd
  Fast-forward
   gamma | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 gamma
  $ cd ../..
  $ echo xxx >> alpha
  $ hg commit -m 'Update subrepo2 from hg' | grep -v "committing subrepository" || true
  $ hg push
  pushing to $TESTTMP/gitrepo1
  searching for changes
  adding objects
  added 1 commits with 2 trees and 1 blobs
  updating reference refs/heads/master
  $ cd ..
  $ cd gitrepo1
there shall be two gitlink entries, with values matching that in .hgsubstate
  $ git ls-tree -r HEAD^{tree} | grep 'commit'
  160000 commit 56f0304c5250308f14cfbafdc27bd12d40154d17	subrepo1
  160000 commit aabf7cd015089aff0b84596e69aa37b24a3d090a	xyz/subrepo2
bring working copy to HEAD state (it's not bare repo)
  $ git reset --hard
  HEAD is now at 4663c49 Update subrepo2 from hg
  $ cd ..

3. Check .hgsub and .hgsubstate from git repository are merged, not overwritten
  $ hg init hgsub
  $ cd hgsub
  $ echo delta > delta
  $ hg add delta
  $ fn_hg_commit -m "add delta"
  $ echo "`hg tip --template '{node}'` hgsub" > ../gitrepo1/.hgsubstate
  $ echo "hgsub = $(pwd)" > ../gitrepo1/.hgsub
  $ cd ../gitrepo1
  $ git add .hgsubstate .hgsub
  $ fn_git_commit -m "Test3. Prepare .hgsub and .hgsubstate sources"
  $ cd ../hgrepo
  $ hg pull
  pulling from $TESTTMP/gitrepo1
  importing git objects into hg
  (run 'hg update' to get a working copy)
We grep off the updating to active bookmark message from newer versions. When
only 2.7 or later (or so) is supported, this grep should be dropped.
  $ hg checkout -C | grep -v 'updating to active bookmark master' | egrep -v '^\(activating bookmark master\)$' 
  cloning subrepo hgsub from $TESTTMP/hgsub
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..
pull shall bring .hgsub entry which was added to the git repo
  $ cat hgrepo/.hgsub | sort
  hgsub = $TESTTMP/hgsub
  subrepo1 = [git]../gitsubrepo
  xyz/subrepo2 = [git]../gitsubrepo
.hgsubstate shall list revision of the subrepo added through git repo
  $ cat hgrepo/.hgsubstate
  481ec30d580f333ae3a77f94c973ce37b69d5bda hgsub
  56f0304c5250308f14cfbafdc27bd12d40154d17 subrepo1
  aabf7cd015089aff0b84596e69aa37b24a3d090a xyz/subrepo2
