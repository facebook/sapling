  $ setconfig extensions.treemanifest=!
  $ . helpers-usechg.sh

Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > EOF

Set up server repository

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ echo foo > a
  $ echo foo > b
  $ hg commit -Am 'initial'
  adding a
  adding b
  $ hg book master
  $ cd ..

Set up client repository 1 with pushrebase enabled

  $ hg clone -q ssh://user@dummy/server client1
  $ cd client1
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ cd ..

Set up client repository 2 with pushrebase enabled / fastmanifest enabled

  $ hg clone -q ssh://user@dummy/server client2
  $ cd client2
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > pushrebase=
  > fastmanifest=
  > EOF
  $ cd ..

Create the dummy commit on client 1

  $ cd client1
  $ hg book mybook
  $ echo "text" >> newfile
  $ hg add newfile
  $ hg commit -m 'dummy commit'

Test that pushing to a remotename gets rebased (client1 -> client2) works

  $ hg push --to mybook ssh://user@dummy/client2
  pushing to ssh://user@dummy/client2
  searching for changes
  remote: pushing 1 changeset:
  remote:     eb7a4df38d10  dummy commit

  $ cd ../client2
  $ hg log -G
  o  changeset:   1:eb7a4df38d10
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     dummy commit
  |
  @  changeset:   0:2bb9d20e471c
     bookmark:    master
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initial
  
