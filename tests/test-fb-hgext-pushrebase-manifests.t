This test does two things:

1/ Simulate a common condition of pushrebase under load. Normally pushrebase
caches data before acquiring the write lock (if lazy locking is enabled).
Under load, however, when a push has to wait for the lock more often than not,
much of this data becomes out of date and must be refetched once the lock is
acquired. This test simulates that particular case.
Specifically, we create two clients, client1 and client2, both with
nonconflicting changesets to push. client1's push is artificially blocked by a
`prepushrebase` hook (post-caching, pre-lock) that is only released after
client2's push succeeds.

2/ Checks how often we call manifest.read() inside the lock (and outside).

This way we can prevent regressions on manifest reads and test improvements.
manifest.read() is wrapped by an extension that prints a short trace. read calls
inside the lock are marked with a ":(".

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > username = nobody <no.reply@fb.com>
  > [extensions]
  > strip =
  > EOF

  $ commit() {
  >   hg commit -A -m "$@"
  > }

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{node|short}] {bookmarks}" "$@"
  > }
  $ config() {
  >   echo "[experimental]" >> .hg/hgrc
  >   echo "bundle2lazylocking=True" >> .hg/hgrc
  >   echo "[extensions]" >> .hg/hgrc
  >   echo "pushrebase =" >> .hg/hgrc
  > }

  $ clone() {
  >   hg clone ssh://user@dummy/server $1 -q
  >   cd $1
  >   config
  > }

Set up server repository

  $ hg init server
  $ cd server
  $ config
  $ echo foo > base
  $ commit "[base] (zero'th)"
  adding base

Clone client1 and client2 from the server repo.

  $ cd ..
  $ clone client1
  $ cd ..
  $ clone client2

Make some non-conflicting commits in all three repos.
  $ cd ../server
  $ echo 'bar' > srv
  $ commit 'srv => bar (first)'
  adding srv
  $ log
  @  srv => bar (first) [draft:2d83594e8405]
  |
  o  [base] (zero'th) [draft:a9156650d8dd]
  
  $ cd ../client1
  $ echo 'xxx' > c1
  $ commit 'c1 => xxx (third)'
  adding c1
  $ echo 'baz' > c1
  $ commit 'c1 => baz (fourth)'
  $ log
  @  c1 => baz (fourth) [draft:1fe62957ca8a]
  |
  o  c1 => xxx (third) [draft:8cf3b846b3a4]
  |
  o  [base] (zero'th) [public:a9156650d8dd]
  
  $ cd ../client2
  $ log
  @  [base] (zero'th) [public:a9156650d8dd]
  
  $ echo 'yyy' > c2
  $ commit 'c2 => yyy (second)'
  adding c2

Add an extension that logs whenever `manifest.readmf()` is called when the lock is held.
  $ cat >> $TESTTMP/manifestcheck.py <<EOF
  > import sys, traceback, os
  > from edenscm.mercurial import extensions, manifest
  > from edenscm.mercurial.node import nullrev
  > def uisetup(ui):
  >     extensions.wrapfunction(manifest.manifestrevlog, 'revision', readmf)
  > def readmf(orig, self, nodeorrev, **kwargs):
  >     haslock = False
  >     try:
  >       haslock = os.path.lexists(os.path.join(self.opener.join(''), "../wlock"))
  >     except Exception as e:
  >       print >> sys.stderr, 'manifest: %s' % e
  >       pass
  >     if nodeorrev != nullrev:
  >         if haslock:
  >           print >> sys.stderr, 'read flat manifest :('
  >           stack = traceback.extract_stack()
  >           # Uncomment for context:
  >           # print >> sys.stderr, ''.join(traceback.format_list(stack[-5:-3]))
  >         else:
  >           print >> sys.stderr, "read manifest outside the lock :)"
  >     return orig(self, nodeorrev, **kwargs)
  > EOF

Add a hook to the server to make it spin until .hg/flag exists.
  $ cd ../server
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "manifestcheck=$TESTTMP/manifestcheck.py" >> .hg/hgrc
  $ cp .hg/hgrc .hg/hgrc.bak
  $ echo "[hooks]" >> .hg/hgrc
  $ echo "prepushrebase.wait=python:$TESTDIR/hgsql/waithook.py:waithook" >> .hg/hgrc

Push from client1 -> server and detach. The background job will wait for
.hg/flag.
  $ cd ../client1
  $ hg push --to default 2>&1 | \sed "s/^/[client1 push] /" &

Wait for the first push to actually enter the hook before removing it.
  $ cd ../server
  $ while [ ! -f ".hg/hookrunning" ]; do sleep 0.01; done

Remove the hook.
  $ cp .hg/hgrc.bak .hg/hgrc

Push from client2 -> server. This should go through immediately without
blocking. There shouldn't be any "[client1 push]" output here.
  $ cd ../client2
  $ hg push --to default 2>&1 | \sed "s/^/[client2 push] /"
  [client2 push] remote:  (?)
  [client2 push] remote:  (?)
  [client2 push] pushing to ssh://user@dummy/server
  [client2 push] searching for changes
  [client2 push] remote: read manifest outside the lock :)
  [client2 push] remote: read manifest outside the lock :)
  [client2 push] remote:  (?)
  [client2 push] remote:  (?)
  [client2 push] remote: read manifest outside the lock :)
  [client2 push] remote: read manifest outside the lock :)
  [client2 push] remote: pushing 1 changeset:
  [client2 push] remote:     4ab7e28729f6  c2 => yyy (second)
  [client2 push] remote: 2 new changesets from the server will be downloaded
  [client2 push] adding changesets
  [client2 push] adding manifests
  [client2 push] adding file changes
  [client2 push] added 2 changesets with 1 changes to 2 files (+1 heads)
  [client2 push] 1 new obsolescence markers
  [client2 push] obsoleted 1 changesets
  [client2 push] 1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log
  @  c2 => yyy (second) [public:d732e3c60e5e]
  |
  o  srv => bar (first) [public:2d83594e8405]
  |
  o  [base] (zero'th) [public:a9156650d8dd]
  

Check that the first push is still running/blocked...
  $ jobs
  [1]+  Running                 hg push --to default 2>&1 | \sed "s/^/[client1 push] /" &  (wd: ~/client1)
...then allow it through.
  $ cd ../server
  $ touch .hg/flag
  $ wait
  [client1 push] pushing to ssh://user@dummy/server
  [client1 push] searching for changes
  [client1 push] remote: read manifest outside the lock :)
  [client1 push] remote: read manifest outside the lock :)
  [client1 push] remote:  (?)
  [client1 push] remote:  (?)
  [client1 push] remote:  (?)
  [client1 push] remote: read manifest outside the lock :)
  [client1 push] remote: read manifest outside the lock :)
  [client1 push] remote: pushing 2 changesets:
  [client1 push] remote:     8cf3b846b3a4  c1 => xxx (third)
  [client1 push] remote:     1fe62957ca8a  c1 => baz (fourth)
  [client1 push] remote: read flat manifest :(
  [client1 push] remote: read flat manifest :(
  [client1 push] remote: 4 new changesets from the server will be downloaded
  [client1 push] adding changesets
  [client1 push] adding manifests
  [client1 push] adding file changes
  [client1 push] added 4 changesets with 2 changes to 3 files (+1 heads)
  [client1 push] 2 new obsolescence markers
  [client1 push] obsoleted 2 changesets
  [client1 push] 2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Verify the proper commit order. (Note: The received commits here shouldn't be
draft; see t16967599).
  $ cd ../client1 && log
  @  c1 => baz (fourth) [public:24b9fc6d79e7]
  |
  o  c1 => xxx (third) [public:074726aeb626]
  |
  o  c2 => yyy (second) [public:d732e3c60e5e]
  |
  o  srv => bar (first) [public:2d83594e8405]
  |
  o  [base] (zero'th) [public:a9156650d8dd]
  
client2 should only have its changesets because it won:
  $ cd ../client2 && log
  @  c2 => yyy (second) [public:d732e3c60e5e]
  |
  o  srv => bar (first) [public:2d83594e8405]
  |
  o  [base] (zero'th) [public:a9156650d8dd]
  

