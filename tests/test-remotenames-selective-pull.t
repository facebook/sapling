  $ setconfig extensions.treemanifest=!
Set up repos

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh=python "$TESTDIR/dummyssh"
  > [extensions]
  > remotenames=
  > EOF
  $ hg init remoterepo
  $ hg clone -q ssh://user@dummy/remoterepo localrepo

Pull master bookmark

  $ cd remoterepo
  $ echo a > a
  $ hg add a
  $ hg commit -m 'First'
  $ hg book master
  $ cd ../localrepo
  $ hg pull
  pulling from ssh://user@dummy/remoterepo
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 1449e7934ec1
  $ hg bookmarks --remote
     default/master            0:1449e7934ec1

Set up selective pull
  $ cat >> .hg/hgrc << EOF
  > [remotenames]
  > selectivepull=True
  > selectivepulldefault=master
  > EOF

Create another bookmark on the remote repo
  $ cd ../remoterepo
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark master)
  $ hg book secondbook
  $ echo b >> a
  $ hg add a
  $ hg commit -m 'Commit secondbook points to'

Do not pull new boookmark from local repo
  $ cd ../localrepo
  $ hg pull
  pulling from ssh://user@dummy/remoterepo
  no changes found
  $ hg bookmarks --remote
     default/master            0:1449e7934ec1

Do not pull new bookmark even if it on the same commit as old bookmark
  $ cd ../remoterepo
  $ hg up -q master
  $ hg book thirdbook
  $ cd ../localrepo
  $ hg pull
  pulling from ssh://user@dummy/remoterepo
  no changes found
  $ hg bookmarks --remote
     default/master            0:1449e7934ec1

Move master bookmark
  $ cd ../remoterepo
  $ hg up -q master
  $ echo a >> a
  $ hg commit -m 'Move master bookmark'
  $ cd ../localrepo
  $ hg pull
  pulling from ssh://user@dummy/remoterepo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 0238718db2b1
  $ hg bookmarks --remote
     default/master            1:0238718db2b1

Specify bookmark to pull
  $ hg pull -B secondbook
  pulling from ssh://user@dummy/remoterepo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets ed7a9fd254d1
  $ hg bookmarks --remote
     default/master            1:0238718db2b1
     default/secondbook        2:ed7a9fd254d1

Create second remote
  $ cd ..
  $ hg clone -q ssh://user@dummy/remoterepo secondremoterepo
  $ cd secondremoterepo
  $ hg up -q 0238718db2b1
  $ hg book master --force
  $ cd ..

Add second remote repo path in localrepo
  $ cd localrepo
  $ cat >> $HGRCPATH << EOF
  > [paths]
  > secondremote=ssh://user@dummy/secondremoterepo
  > EOF
  $ hg pull secondremote
  pulling from ssh://user@dummy/secondremoterepo
  no changes found
  $ hg book --remote
     default/master            1:0238718db2b1
     default/secondbook        2:ed7a9fd254d1
     secondremote/master       1:0238718db2b1

Move bookmark in second remote, pull and make sure it doesn't move in local repo
  $ cd ../secondremoterepo
  $ hg book secondbook --force
  $ echo aaa >> a
  $ hg commit -m 'Move bookmark in second remote'
  $ cd ../localrepo
  $ hg pull secondremote
  pulling from ssh://user@dummy/secondremoterepo
  no changes found

Move bookmark in first remote, pull and make sure it moves in local repo
  $ cd ../remoterepo
  $ hg up -q secondbook
  $ echo bbb > a
  $ hg commit -m 'Moves second bookmark'
  $ cd ../localrepo
  $ hg pull
  pulling from ssh://user@dummy/remoterepo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets c47dca9795c9
  $ hg bookmarks --remote
     default/master            1:0238718db2b1
     default/secondbook        3:c47dca9795c9
     secondremote/master       1:0238718db2b1

Delete bookmark on the server
  $ cd ../remoterepo
  $ hg book -d secondbook
  $ cd ../localrepo
  $ hg pull
  pulling from ssh://user@dummy/remoterepo
  no changes found
  $ hg bookmarks --remote
     default/master            1:0238718db2b1
     secondremote/master       1:0238718db2b1

Update to the remote bookmark
  $ hg update thirdbook
  `thirdbook` not found: assuming it is a remote bookmark and trying to pull it
  pulling from ssh://user@dummy/remoterepo
  no changes found
  `thirdbook` found remotely
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg book --verbose
  no bookmarks set
  $ hg book --remote
     default/master            1:0238718db2b1
     default/thirdbook         0:1449e7934ec1
     secondremote/master       1:0238718db2b1

Trying to update to unknown bookmark
  $ hg update unknownbook
  `unknownbook` not found: assuming it is a remote bookmark and trying to pull it
  pulling from ssh://user@dummy/remoterepo
  pull failed: remote bookmark unknownbook not found!
  abort: unknown revision 'unknownbook'!
  (if unknownbook is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

Update to the remote bookmark from secondremote
  $ hg update secondremote/secondbook
  `secondremote/secondbook` not found: assuming it is a remote bookmark and trying to pull it
  pulling from ssh://user@dummy/secondremoterepo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 0022441e80e5
  `secondremote/secondbook` found remotely
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg book --remote
     default/master            1:0238718db2b1
     default/thirdbook         0:1449e7934ec1
     secondremote/master       1:0238718db2b1
     secondremote/secondbook   4:0022441e80e5

Update make sure revsets work
  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Make another clone with selectivepull disabled
  $ hg clone -q ssh://user@dummy/remoterepo localrepo2
  $ cd localrepo2
  $ hg book --remote
     default/master            2:0238718db2b1
     default/thirdbook         0:1449e7934ec1

Enable selectivepull and make a pull. Make sure only master bookmark is left
  $ cat >> .hg/hgrc << EOF
  > [remotenames]
  > selectivepull=True
  > selectivepulldefault=master
  > EOF
  $ hg pull -q
  $ hg book --remote
     default/master            2:0238718db2b1

Temporarily disable selectivepull, pull, enable it again and pull again.
Make sure only master bookmark is present
  $ hg pull --config remotenames.selectivepull=False -q
  $ hg book --remote
     default/master            2:0238718db2b1
     default/thirdbook         0:1449e7934ec1
  $ hg pull -q
  $ hg book --remote
     default/master            2:0238718db2b1

Check that log shows the hint about selective pull
  $ hg log -r default/thirdbook
  abort: unknown revision 'default/thirdbook'!
  (if default/thirdbook is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

Set two bookmarks in selectivepulldefault, make sure both of them were pulled
  $ cat >> .hg/hgrc << EOF
  > [remotenames]
  > selectivepulldefault=master,thirdbook
  > EOF
  $ rm .hg/selectivepullenabled
  $ hg pull -q
  $ hg book --remote
     default/master            2:0238718db2b1
     default/thirdbook         0:1449e7934ec1

Check subscriptions flag
  $ hg book --list-subscriptions
     default/master            2:0238718db2b1
     default/thirdbook         0:1449e7934ec1
