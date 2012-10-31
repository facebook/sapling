Load commonly used test logic
  $ . "$TESTDIR/testutil"

bail if the user does not have git command-line client
  $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m "add alpha"
  $ git branch alpha
  $ git show-ref
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 refs/heads/alpha
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 refs/heads/master

  $ cd ..
  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd hgrepo
  $ hg update -q master
  $ echo beta > beta
  $ hg add beta
  $ fn_hg_commit -m 'add beta'


  $ echo gamma > gamma
  $ hg add gamma
  $ fn_hg_commit -m 'add gamma'

  $ hg book -r 1 beta

  $ hg outgoing | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with */gitrepo (glob)
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  changeset:   2:72f56395749d
  tag:         master
  tag:         tip
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  
  $ hg outgoing -r beta | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with */gitrepo (glob)
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  $ hg outgoing -r master | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with */gitrepo (glob)
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  changeset:   2:72f56395749d
  tag:         master
  tag:         tip
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  

  $ cd ..

  $ echo % some more work on master from git
  % some more work on master from git
  $ cd gitrepo

Check state of refs after outgoing
  $ git show-ref
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 refs/heads/alpha
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 refs/heads/master

  $ git checkout master 2>&1 | sed s/\'/\"/g
  Already on "master"
  $ echo delta > delta
  $ git add delta
  $ fn_git_commit -m "add delta"

  $ cd ..

  $ cd hgrepo
  $ echo % this will fail # maybe we should try to make it work
  % this will fail
  $ hg outgoing
  comparing with */gitrepo (glob)
  abort: refs/heads/master changed on the server, please pull and merge before pushing
  [255]
  $ echo % let\'s pull and try again
  % let's pull and try again
  $ hg pull 2>&1 | grep -v 'divergent bookmark'
  pulling from */gitrepo (glob)
  importing git objects into hg
  (run 'hg update' to get a working copy)
  $ hg outgoing | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with */gitrepo (glob)
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  changeset:   2:72f56395749d
  tag:         master
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  
  $ hg outgoing -r beta | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with */gitrepo (glob)
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  $ hg outgoing -r master | sed 's/bookmark:    /tag:         /' | grep -v 'searching for changes'
  comparing with */gitrepo (glob)
  changeset:   1:0564f526fb0f
  tag:         beta
  user:        test
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  changeset:   2:72f56395749d
  tag:         master
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  


  $ cd ..
