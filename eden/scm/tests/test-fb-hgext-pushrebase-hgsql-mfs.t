#chg-compatible

TODO: Make this test work with obsstore
  $ disable treemanifest
  $ setconfig experimental.evolution=
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
  >   setconfig pushrebase.debugprintmanifestreads=True
  >   setconfig pushrebase.debugprintmanifestreads.user=True
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
  $ cd ..

  $ newrepo server2
  $ config_server

Clone client1 and client2 from the server repo.

  $ cd ..
  $ clone client1
  $ cd ..
  $ clone client2 server2

Make some non-conflicting commits in in the client repos.

  $ cd ../client1
  $ echo 'xxx' > c1
  $ commit 'first commit'
  *FULL* manifest read for 1e4ac5512124 (*inside* lock)
  $ echo 'baz' > c1
  $ commit 'second commit'
  *FULL* manifest read for bef5a3bb1f24 (*inside* lock)
  $ log
  @  second commit [draft:0a57cb610829] master
  |
  o  first commit [draft:679b2ce82944]
  |
  o  base [public:4ced94c0a443]
  
  $ hg push -q -r tip --to master
  $ log
  @  second commit [public:0a57cb610829] master
  |
  o  first commit [public:679b2ce82944]
  |
  o  base [public:4ced94c0a443]
  

  $ cd ../client2
  $ hg pull -q
  $ log
  o  second commit [public:0a57cb610829] master
  |
  o  first commit [public:679b2ce82944]
  |
  @  base [public:4ced94c0a443]
  
  $ echo 'yyy' > c2
  $ commit 'third commit'
  *FULL* manifest read for 1e4ac5512124 (*inside* lock)
  $ log
  @  third commit [draft:8ee8e01cbc17]
  |
  | o  second commit [public:0a57cb610829] master
  | |
  | o  first commit [public:679b2ce82944]
  |/
  o  base [public:4ced94c0a443]
  
  $ hg push -r . --to master
  pushing to ssh://user@dummy/server2
  searching for changes
  remote: *FULL* manifest read for 1e4ac5512124 (outside lock)
  remote: cached manifest read for 1e4ac5512124 (outside lock)
  remote: cached manifest read for 1e4ac5512124 (outside lock)
  remote: *FULL* manifest read for 8655e3409b0e (outside lock)
  remote: pushing 1 changeset:
  remote:     8ee8e01cbc17  third commit
  remote: 1 new changeset from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files
  $ hg pull -q
  $ log
  o  third commit [public:e71dff9ebf0e]
  |
  | @  third commit [draft:8ee8e01cbc17]
  | |
  o |  second commit [public:0a57cb610829] master
  | |
  o |  first commit [public:679b2ce82944]
  |/
  o  base [public:4ced94c0a443]
  
The two server repos should look identical:

  $ cd ../server1
  $ log
  o  third commit [public:e71dff9ebf0e]
  |
  o  second commit [public:0a57cb610829] master
  |
  o  first commit [public:679b2ce82944]
  |
  @  base [public:4ced94c0a443]
  
  $ cd ../server2
  $ log
  o  third commit [public:e71dff9ebf0e]
  |
  o  second commit [public:0a57cb610829] master
  |
  o  first commit [public:679b2ce82944]
  |
  o  base [public:4ced94c0a443]
  
Add a hook to the server to make it spin until .hg/flag exists:

  $ cd ../server1
  $ cp .hg/hgrc .hg/hgrc.bak
  $ echo "[hooks]" >> .hg/hgrc
  $ echo "prepushrebase.wait=python:$TESTDIR/hgsql/waithook.py:waithook" >> .hg/hgrc


Push from client1 -> server1 and detach. The background job will wait for
.hg/flag.

  $ cd ../client1
  $ echo 'yyy' > d1
  $ commit 'race loser'
  *FULL* manifest read for 8655e3409b0e (*inside* lock)
  $ log
  @  race loser [draft:0ee934622ec8] master
  |
  o  second commit [public:0a57cb610829]
  |
  o  first commit [public:679b2ce82944]
  |
  o  base [public:4ced94c0a443]
  
  $ hg push --to master 2>&1 | \sed "s/^/[client1 push] /" &

Wait for the first push to actually enter the hook before removing it.
  $ cd ../server1
  $ while [ ! -f ".hg/hookrunning" ]; do sleep 0.01; done

Meanwhile, push from client2 -> server2.
  $ cd ../client2
  $ echo 'qqq' > d2
  $ commit 'race winner'
  *FULL* manifest read for 46410a8f6645 (*inside* lock)
  $ log
  @  race winner [draft:0e59db56ba07]
  |
  | o  third commit [public:e71dff9ebf0e]
  | |
  o |  third commit [draft:8ee8e01cbc17]
  | |
  | o  second commit [public:0a57cb610829] master
  | |
  | o  first commit [public:679b2ce82944]
  |/
  o  base [public:4ced94c0a443]
  
  $ hg push --to master 2>&1 | \sed "s/^/[client2 push] /"
  [client2 push] pushing to ssh://user@dummy/server2
  [client2 push] searching for changes
  [client2 push] remote: *FULL* manifest read for 1e4ac5512124 (outside lock)
  [client2 push] remote: cached manifest read for 1e4ac5512124 (outside lock)
  [client2 push] remote: cached manifest read for 1e4ac5512124 (outside lock)
  [client2 push] remote: *FULL* manifest read for 8655e3409b0e (outside lock)
  [client2 push] remote: pushing 2 changesets:
  [client2 push] remote:     8ee8e01cbc17  third commit
  [client2 push] remote:     0e59db56ba07  race winner
  [client2 push] remote: *FULL* manifest read for 0bf21a535a1f (*inside* lock)
  [client2 push] remote: 1 new changeset from the server will be downloaded
  [client2 push] adding changesets
  [client2 push] adding manifests
  [client2 push] adding file changes
  [client2 push] added 1 changesets with 0 changes to 1 files

Check that the first push is still running/blocked...
  $ jobs
  [1]+  Running                 hg push --to master 2>&1 | \sed "s/^/[client1 push] /" &  (wd: ~/client1)

...then allow it through.
  $ cd ../server1
  $ touch .hg/flag
  $ wait
  [client1 push] pushing to ssh://user@dummy/server1
  [client1 push] searching for changes
  [client1 push] remote: *FULL* manifest read for 8655e3409b0e (outside lock)
  [client1 push] remote: cached manifest read for 8655e3409b0e (outside lock)
  [client1 push] remote: cached manifest read for 8655e3409b0e (outside lock)
  [client1 push] remote: pushing 1 changeset:
  [client1 push] remote:     0ee934622ec8  race loser
  [client1 push] remote: cached manifest read for 8655e3409b0e (*inside* lock)
  [client1 push] updating bookmark master
  $ log
  o  race loser [public:0ee934622ec8] master
  |
  | o  race winner [public:f265c7c9f0c9]
  | |
  | o  third commit [public:e71dff9ebf0e]
  |/
  o  second commit [public:0a57cb610829]
  |
  o  first commit [public:679b2ce82944]
  |
  @  base [public:4ced94c0a443]
  

  $ cd ../server2
  $ log
  o  race loser [public:0ee934622ec8] master
  |
  | o  race winner [public:f265c7c9f0c9]
  | |
  | o  third commit [public:e71dff9ebf0e]
  |/
  o  second commit [public:0a57cb610829]
  |
  o  first commit [public:679b2ce82944]
  |
  o  base [public:4ced94c0a443]
  
