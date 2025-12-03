#require git no-windows

  $ eagerepo
  $ enable tweakdefaults
  $ setconfig diff.git=True
  $ setconfig subtree.min-path-depth=1
  $ setconfig tweakdefaults.logdefaultfollow=True

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

Test log support subtree import

  $ hg subtree import -q --url $GIT_URL --rev main --to-path bar -m "import gitrepo to bar"
  $ echo 3 >> bar/alpha
  $ hg ci -m "update bar/alpha"

  $ hg log bar/alpha
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     update bar/alpha
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     import gitrepo to bar
  
  commit:      6a5b13188f04~
  bookmark:    remote/main
  hoistedname: main
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:10 2007 +0000
  summary:     update alpha\nhttps://phabricator.test.com/D1234567
  
  commit:      b6c31add3e60~
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:10 2007 +0000
  summary:     alpha

Test log with --limit

  $ hg log bar/alpha --limit 2
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     update bar/alpha
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     import gitrepo to bar

  $ hg log bar/alpha --limit 3
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     update bar/alpha
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     import gitrepo to bar
  
  commit:      6a5b13188f04~
  bookmark:    remote/main
  hoistedname: main
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:10 2007 +0000
  summary:     update alpha\nhttps://phabricator.test.com/D1234567

Test commit color

  $ hg log bar/alpha --color=debug
  [log.changeset changeset.draft|commit:      *] (glob)
  [log.user|user:        test]
  [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
  [log.summary|summary:     update bar/alpha]
  
  [log.changeset changeset.draft|commit:      *] (glob)
  [log.user|user:        test]
  [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
  [log.summary|summary:     import gitrepo to bar]
  
  [log.changeset changeset.public changeset.xrepo|commit:      6a5b13188f04~]
  [log.remotebookmark|bookmark:    remote/main]
  [log.hoistedname|hoistedname: main]
  [log.user|user:        test <test@example.org>]
  [log.date|date:        Mon Jan 01 00:00:10 2007 +0000]
  [log.summary|summary:     update alpha\nhttps://phabricator.test.com/D1234567]
  
  [log.changeset changeset.public changeset.xrepo|commit:      b6c31add3e60~]
  [log.user|user:        test <test@example.org>]
  [log.date|date:        Mon Jan 01 00:00:10 2007 +0000]
  [log.summary|summary:     alpha]

Test xreponame keyword
  $ hg log bar/alpha -T '{xreponame}\n'
  
  
  gitrepo
  gitrepo

Test log with --graph
  $ hg log bar/alpha --graph
  @  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     update bar/alpha
  │
  o  commit:      * (glob)
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:00 1970 +0000
  ╷  summary:     import gitrepo to bar
  ╷
  @  commit:      6a5b13188f04~
  │  user:        test <test@example.org>
  │  date:        Mon Jan 01 00:00:10 2007 +0000
  │  summary:     update alpha\nhttps://phabricator.test.com/D1234567
  │
  o  commit:      b6c31add3e60~
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     alpha

Test commit color with --graph

  $ hg log bar/alpha --graph --color=debug
  @  [log.changeset changeset.draft|commit:      *] (glob)
  │  [log.user|user:        test]
  │  [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
  │  [log.summary|summary:     update bar/alpha]
  │
  o  [log.changeset changeset.draft|commit:      *] (glob)
  ╷  [log.user|user:        test]
  ╷  [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
  ╷  [log.summary|summary:     import gitrepo to bar]
  ╷
  @  [log.changeset changeset.public changeset.xrepo|commit:      6a5b13188f04~]
  │  [log.user|user:        test <test@example.org>]
  │  [log.date|date:        Mon Jan 01 00:00:10 2007 +0000]
  │  [log.summary|summary:     update alpha\nhttps://phabricator.test.com/D1234567]
  │
  o  [log.changeset changeset.public changeset.xrepo|commit:      b6c31add3e60~]
     [log.user|user:        test <test@example.org>]
     [log.date|date:        Mon Jan 01 00:00:10 2007 +0000]
     [log.summary|summary:     alpha]

Test log.follow-xrepo config

  $ hg log bar/alpha --config log.follow-xrepo=False
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     update bar/alpha
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     import gitrepo to bar
  
  $ hg log bar/alpha --graph --config log.follow-xrepo=False
  @  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     update bar/alpha
  │
  o  commit:      * (glob)
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     import gitrepo to bar
