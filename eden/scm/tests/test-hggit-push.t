Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m "add alpha"
  $ git checkout -b not-master
  Switched to a new branch 'not-master'

  $ cd ..
  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd hgrepo
  $ hg bookmark -q master
  $ echo beta > beta
  $ hg add beta
  $ fn_hg_commit -m 'add beta'


  $ echo gamma > gamma
  $ hg add gamma
  $ fn_hg_commit -m 'add gamma'

  $ hg book -r 1 beta
  $ hg push -r beta
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  adding reference refs/heads/beta

  $ cd ..

should have two different branches
  $ cd gitrepo
  $ git branch -v
    beta       0f378ab add beta
    master     7eeab2e add alpha
  * not-master 7eeab2e add alpha

some more work on master from git
  $ git checkout master
  Switched to branch 'master'
  $ echo delta > delta
  $ git add delta
  $ fn_git_commit -m "add delta"
  $ git checkout not-master
  Switched to branch 'not-master'

  $ cd ..

  $ cd hgrepo
this should fail
  $ hg push -r master
  pushing to $TESTTMP/gitrepo
  searching for changes
  abort: branch 'refs/heads/master' changed on the server, please pull and merge before pushing
  [255]

... even with -f
  $ hg push -fr master
  pushing to $TESTTMP/gitrepo
  searching for changes
  abort: branch 'refs/heads/master' changed on the server, please pull and merge before pushing
  [255]

  $ hg pull 2>&1 | grep -v 'divergent bookmark'
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
TODO shouldn't need to do this since we're (in theory) pushing master explicitly,
which should not implicitly also push the not-master ref.
  $ hg book not-master -r default/not-master --force
master and default/master should be diferent
  $ hg log -r master
  changeset:   2:49480a0fbf45
  bookmark:    master
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  
  $ hg log -r default/master
  changeset:   3:59db0d26c08d
  tag:         default/master
  tag:         tip
  parent:      0:69982ec78c6d
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:13 2007 +0000
  summary:     add delta
  

this should also fail
  $ hg push -r master
  pushing to $TESTTMP/gitrepo
  searching for changes
  abort: pushing refs/heads/master overwrites 49480a0fbf45
  [255]

... but succeed with -f
  $ hg push -fr master
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  updating reference refs/heads/master

this should fail, no changes to push
  $ hg push -r master
  pushing to $TESTTMP/gitrepo
  searching for changes
  no changes found
  [1]

hg-git issue103 -- directories can lose information at hg-git export time

  $ hg up master | egrep -v '^\(activating bookmark master\)$'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir dir1
  $ echo alpha > dir1/alpha
  $ hg add dir1/alpha
  $ fn_hg_commit -m 'add dir1/alpha'
  $ hg push -r master
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 2 trees and 0 blobs
  updating reference refs/heads/master

  $ echo beta > dir1/beta
  $ hg add dir1/beta
  $ fn_hg_commit -m 'add dir1/beta'
  $ hg push -r master
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 2 trees and 0 blobs
  updating reference refs/heads/master
  $ hg log -r master
  changeset:   5:e4281e9db8f8
  bookmark:    master
  tag:         default/master
  tag:         tip
  user:        test
  date:        Mon Jan 01 00:00:15 2007 +0000
  summary:     add dir1/beta
  

  $ cd ..

  $ hg clone gitrepo hgrepo-test
  importing git objects into hg
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo-test log -r master
  changeset:   4:8df619e46009
  bookmark:    master
  tag:         default/master
  tag:         tip
  user:        test
  date:        Mon Jan 01 00:00:15 2007 +0000
  summary:     add dir1/beta
  

Push empty Hg repo to empty Git repo (issue #58)
  $ hg init hgrepo2
  $ git init -q --bare gitrepo2
  $ hg -R hgrepo2 push gitrepo2
  pushing to gitrepo2
  searching for changes
  no changes found
  [1]

The remote repo is empty and the local one doesn't have any bookmarks/tags
  $ cd hgrepo2
  $ echo init >> test.txt
  $ hg addremove
  adding test.txt
  $ fn_hg_commit -m init
  $ hg update null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
(the phases line was added in Mercurial 3.5)
  $ hg summary | grep -Ev '^phases:'
  parent: -1:000000000000  (no revision checked out)
  commit: (clean)
Only one bookmark 'master' should be created
  $ hg bookmarks
   * master                    0:8aded40be5af

test for ssh vulnerability

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh = ssh -o ConnectTimeout=1
  > EOF
  $ hg push 'git+ssh://-oProxyCommand=rm${IFS}nonexistent/path' 2>&1 >/dev/null
  abort: potentially unsafe hostname: '-oProxyCommand=rm${IFS}nonexistent'
  [255]
  $ hg push 'git+ssh://-oProxyCommand=rm%20nonexistent/path' 2>&1 >/dev/null
  abort: potentially unsafe hostname: '-oProxyCommand=rm nonexistent'
  [255]
  $ hg push 'git+ssh://fakehost|rm%20nonexistent/path' 2>&1 >/dev/null
  ssh: .* fakehost%7[Cc]rm%20nonexistent.* (re)
  abort: git remote error: The remote server unexpectedly closed the connection.
  [255]
  $ hg push 'git+ssh://fakehost%7Crm%20nonexistent/path' 2>&1 >/dev/null
  ssh: .* fakehost%7[Cc]rm%20nonexistent.* (re)
  abort: git remote error: The remote server unexpectedly closed the connection.
  [255]
