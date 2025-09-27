#require git no-windows

  $ eagerepo
  $ setconfig diff.git=True
  $ setconfig subtree.min-path-depth=1

Prepare a git repo:

  $ . $TESTDIR/git.sh
  $ git -c init.defaultBranch=main init -q gitrepo
  $ cd gitrepo
  $ git config core.autocrlf false
  $ echo 1 > alpha
  $ git add alpha
  $ git commit -q -malpha
  $ echo 2 >> alpha
  $ git commit -aqm 'update alpha\nhttps://phabricator.test.com/D1234567'

  $ git log --graph
  * commit 6a5b13188f04b7dee69219f6b24d2d1996a60faa
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     update alpha\nhttps://phabricator.test.com/D1234567
  | 
  * commit b6c31add3e60ded7a9c9c803641edffb1dccd251
    Author: test <test@example.org>
    Date:   Mon Jan 1 00:00:10 2007 +0000
    
        alpha

  $ export GIT_URL=git+file://$TESTTMP/gitrepo
  
Prepare a Sapling repo:

  $ newclientrepo
  $ drawdag <<'EOS'
  > A
  > EOS
  $ hg go $A -q

Test blame support subtree import

  $ hg subtree import -q --url $GIT_URL --rev main --to-path bar -m "import gitrepo to bar"
  $ echo 3 >> bar/alpha
  $ hg ci -m "update bar/alpha"
  $ hg blame bar/alpha
  b6c31add3e60~: 1
  6a5b13188f04~: 2
  *: 3 (glob)

Test commit's color
  $ hg blame bar/alpha --color=debug
  [blame.age.old.xrepo|b6c31add3e60~: ]1
  [blame.age.old.xrepo|6a5b13188f04~: ]2
  [blame.age.old|* : ]3 (glob)

Test blame support phabricator diff number
  $ hg blame bar/alpha -p
           : 1
   D1234567: 2
           : 3
