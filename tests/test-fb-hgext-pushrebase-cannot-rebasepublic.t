Demonstrates the "cannot rebase public commits" issue seen using hgsql and
pushrebase.

  $ . "$TESTDIR/hgsql/library.sh"

  $ setconfig ui.ssh='python "$RUNTESTDIR/dummyssh"'
  $ commit() {
  >   hg commit -Aq -m "$@"
  > }

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{node|short}] {bookmarks} {remotenames}" "$@"
  > }

  $ config() {
  >   setconfig experimental.bundle2lazylocking=True
  >   enable pushrebase
  > }

  $ config_server() {
  >   config
  >   configureserver . foo
  > }

  $ clone() { # Usage: "clone <client directory> <source directory>
  >   SRC=${2:-server1}
  >   hg clone ssh://user@dummy/$SRC $1 -q
  >   cd $1
  >   hg up -q master
  >   config
  > }

Set up server repository.

  $ newrepo server1
  $ config_server
  $ echo foo > base
  $ commit "base"
  $ hg book -r . master
  $ log
  @  base [draft:4ced94c0a443] master
  
  $ cd ..

  $ newrepo server2
  $ config_server

Clone client1 from the server repo.

  $ cd ..
  $ clone client1

Make server2 wait:

  $ cd ../server2
  $ cp .hg/hgrc .hg/hgrc.bak
  $ echo "[hooks]" >> .hg/hgrc
  $ echo "prepushrebase.wait=python:$TESTDIR/hgsql/waithook.py:waithook" >> .hg/hgrc

Create two commits in client1:

  $ cd ../client1
  $ echo 'xxx' > c1
  $ commit 'first commit'
  $ echo 'xxx' > c2
  $ commit 'second commit'

Copy client1 to client2:

  $ cd ..
  $ cp -R client1 client2

Push in both repos. Block one of the servers in the prepushrebase hook.
  $ cd client1
  $ hg push --to master ssh://user@dummy/server2 2>&1 |  \sed "s/^/[blocked push] /" &
  $ cd ../client2
  $ hg push --to master ssh://user@dummy/server1 2>&1 |  \sed "s/^/[unblocked push] /"
  [unblocked push] pushing to ssh://user@dummy/server1
  [unblocked push] searching for changes
  [unblocked push] remote: pushing 2 changesets:
  [unblocked push] remote:     679b2ce82944  first commit
  [unblocked push] remote:     aab61efd8449  second commit
  [unblocked push] updating bookmark master

Check that the blocked push is still running/blocked...
  $ jobs
  [1]+  Running                 hg push --to master ssh://user@dummy/server2 2>&1 | \sed "s/^/[blocked push] /" &  (wd: ~/client1)

...then allow it through.
  $ cd ../server2
  $ touch .hg/flag
  $ wait
  [blocked push] pushing to ssh://user@dummy/server2
  [blocked push] searching for changes
  [blocked push] remote: pushing 2 changesets:
  [blocked push] remote:     679b2ce82944  first commit
  [blocked push] remote:     aab61efd8449  second commit
  [blocked push] remote: conflicting changes in:
  [blocked push]     c1
  [blocked push]     c2
  [blocked push] remote: (pull and rebase your changes locally, then try again)
  [blocked push] abort: push failed on remote
  $ log
  o  second commit [public:aab61efd8449] master
  |
  o  first commit [public:679b2ce82944]
  |
  o  base [public:4ced94c0a443]
  
