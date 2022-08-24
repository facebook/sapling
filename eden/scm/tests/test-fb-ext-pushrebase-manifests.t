#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest
  $ enable remotenames
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

  $ configure dummyssh
  $ setconfig ui.username="nobody <no.reply@fb.com>"

  $ commit() {
  >   hg commit -A -m "$@"
  > }

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{node|short}] {bookmarks}" "$@"
  > }
  $ config() {
  >   enable pushrebase
  >   setconfig experimental.bundle2lazylocking=true
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
  $ hg bookmark master

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
  @  srv => bar (first) [draft:2d83594e8405] master
  │
  o  [base] (zero'th) [draft:a9156650d8dd]
  
  $ cd ../client1
  $ echo 'xxx' > c1
  $ commit 'c1 => xxx (third)'
  adding c1
  $ echo 'baz' > c1
  $ commit 'c1 => baz (fourth)'
  $ log
  @  c1 => baz (fourth) [draft:1fe62957ca8a]
  │
  o  c1 => xxx (third) [draft:8cf3b846b3a4]
  │
  o  [base] (zero'th) [public:a9156650d8dd]
  
  $ cd ../client2
  $ log
  @  [base] (zero'th) [public:a9156650d8dd]
  
  $ echo 'yyy' > c2
  $ commit 'c2 => yyy (second)'
  adding c2

Add an extension that logs whenever `manifest.readmf()` is called when the lock is held.
  $ cat >> $TESTTMP/manifestcheck.py <<EOF
  > from __future__ import print_function
  > import sys, traceback, os
  > from edenscm import extensions, manifest
  > from edenscm.node import nullrev
  > def uisetup(ui):
  >     def captureui(*args, **kwargs):
  >         kwargs["ui"] = ui
  >         return readmf(*args, **kwargs)
  >     extensions.wrapfunction(manifest.manifestrevlog, 'revision', captureui)
  > def readmf(orig, self, nodeorrev, **kwargs):
  >     ui = kwargs.pop("ui")
  >     haslock = False
  >     try:
  >       haslock = os.path.lexists(os.path.join(self.opener.join(''), "../wlock"))
  >     except Exception as e:
  >       ui.warn('manifest: %s\n' % e)
  >       pass
  >     if nodeorrev != nullrev:
  >         if haslock:
  >           ui.warn('read flat manifest :(\n')
  >           stack = traceback.extract_stack()
  >           # Uncomment for context:
  >           # print(''.join(traceback.format_list(stack[-5:-3])), file=sys.stderr)
  >         else:
  >           ui.warn("read manifest outside the lock :)\n")
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
  $ hg push --to master 2>&1 | \sed "s/^/[client1 push] /" &

Wait for the first push to actually enter the hook before removing it.
  $ cd ../server
  $ while [ ! -f ".hg/hookrunning" ]; do sleep 0.01; done

Remove the hook.
  $ cp .hg/hgrc.bak .hg/hgrc

Push from client2 -> server. This should go through immediately without
blocking. There shouldn't be any "[client1 push]" output here.
  $ cd ../client2
  $ hg push --to master 2>&1 | \sed "s/^/[client2 push] /"
  [client2 push] remote:  (?)
  [client2 push] remote:  (?)
  [client2 push] pushing rev 4ab7e28729f6 to destination ssh://user@dummy/server bookmark master
  [client2 push] remote:  (?)
  [client2 push] remote:  (?)
  [client2 push] searching for changes
  [client2 push] adding changesets
  [client2 push] adding manifests
  [client2 push] adding file changes
  [client2 push] updating bookmark master
  [client2 push] remote: read manifest outside the lock :)
  [client2 push] remote: read manifest outside the lock :)
  [client2 push] remote: read manifest outside the lock :)
  [client2 push] remote: read manifest outside the lock :)
  [client2 push] remote: pushing 1 changeset:
  [client2 push] remote:     4ab7e28729f6  c2 => yyy (second)
  [client2 push] remote: 2 new changesets from the server will be downloaded
  [client2 push] 1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log
  @  c2 => yyy (second) [public:d732e3c60e5e]
  │
  o  srv => bar (first) [public:2d83594e8405]
  │
  o  [base] (zero'th) [public:a9156650d8dd]
  

Check that the first push is still running/blocked...
  $ jobs
  [1]+  Running                 hg push --to master 2>&1 | \sed "s/^/[client1 push] /" &  (wd: ~/client1)
...then allow it through.
  $ cd ../server
  $ touch .hg/flag
  $ wait
  [client1 push] pushing rev 1fe62957ca8a to destination ssh://user@dummy/server bookmark master
  [client1 push] remote:  (?)
  [client1 push] remote:  (?)
  [client1 push] remote:  (?)
  [client1 push] searching for changes
  [client1 push] adding changesets
  [client1 push] adding manifests
  [client1 push] adding file changes
  [client1 push] updating bookmark master
  [client1 push] remote: read manifest outside the lock :)
  [client1 push] remote: read manifest outside the lock :)
  [client1 push] remote: read manifest outside the lock :)
  [client1 push] remote: read manifest outside the lock :)
  [client1 push] remote: pushing 2 changesets:
  [client1 push] remote:     8cf3b846b3a4  c1 => xxx (third)
  [client1 push] remote:     1fe62957ca8a  c1 => baz (fourth)
  [client1 push] remote: read flat manifest :(
  [client1 push] remote: read flat manifest :(
  [client1 push] remote: 4 new changesets from the server will be downloaded
  [client1 push] 2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Verify the proper commit order. (Note: The received commits here shouldn't be
draft; see t16967599).
  $ cd ../client1 && log
  @  c1 => baz (fourth) [public:24b9fc6d79e7]
  │
  o  c1 => xxx (third) [public:074726aeb626]
  │
  o  c2 => yyy (second) [public:d732e3c60e5e]
  │
  o  srv => bar (first) [public:2d83594e8405]
  │
  o  [base] (zero'th) [public:a9156650d8dd]
  
client2 should only have its changesets because it won:
  $ cd ../client2 && log
  @  c2 => yyy (second) [public:d732e3c60e5e]
  │
  o  srv => bar (first) [public:2d83594e8405]
  │
  o  [base] (zero'th) [public:a9156650d8dd]
  

