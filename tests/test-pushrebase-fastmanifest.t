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
  > bundle2hooks=$TESTDIR/../hgext3rd/bundle2hooks.py
  > pushrebase=$TESTDIR/../hgext3rd/pushrebase.py
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
  > bundle2hooks=$TESTDIR/../hgext3rd/bundle2hooks.py
  > pushrebase=$TESTDIR/../hgext3rd/pushrebase.py
  > EOF
  $ cd ..

Set up client repository 2 with pushrebase enabled / fastmanifest enabled

  $ hg clone -q ssh://user@dummy/server client2
  $ cd client2
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > bundle2hooks=$TESTDIR/../hgext3rd/bundle2hooks.py
  > pushrebase=$TESTDIR/../hgext3rd/pushrebase.py
  > fastmanifest=$TESTDIR/../fastmanifest
  > EOF
  $ cd ..

Create the dummy commit on client 1

  $ cd client1
  $ hg book mybook
  $ echo "text" >> newfile
  $ hg add newfile
  $ hg commit -m 'dummy commit'

Test that pushing to a remotename gets rebased (client1 -> client2), but fails.

  $ hg push --to mybook ssh://user@dummy/client2 2>&1 | tail -4
  remote:   File "*changegroup.py", line *, in seek (glob)
  remote:     return self._stream.seek(pos)
  remote: ValueError: I/O operation on closed file
  abort: stream ended unexpectedly (got 0 bytes, expected 4)

Disable fastmanifest from client2

  $ cd ../client2
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > fastmanifest = !
  > EOF

Test that pushing to a remotename gets rebased (client1 -> client2)

  $ cd ../client1
  $ hg push --to mybook ssh://user@dummy/client2
  pushing to ssh://user@dummy/client2
  searching for changes
  remote: pushing 1 changeset:
  remote:     eb7a4df38d10  dummy commit
