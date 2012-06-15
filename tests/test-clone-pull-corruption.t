Corrupt an hg repo with a pull started during an aborted commit
Create two repos, so that one of them can pull from the other one.

  $ hg init source
  $ cd source
  $ touch foo
  $ hg add foo
  $ hg ci -m 'add foo'
  $ hg clone . ../corrupted
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo >> foo
  $ hg ci -m 'change foo'

Add a hook to wait 5 seconds and then abort the commit

  $ cd ../corrupted
  $ echo "[hooks]" >> .hg/hgrc
  $ echo "pretxncommit = sh -c 'sleep 5; exit 1'" >> .hg/hgrc

start a commit...

  $ touch bar
  $ hg add bar
  $ hg ci -m 'add bar' &

... and start a pull while the commit is still running

  $ sleep 1
  $ hg pull ../source 2>/dev/null
  pulling from ../source
  transaction abort!
  rollback completed
  abort: pretxncommit hook exited with status 1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

see what happened

  $ wait
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 2 changesets, 2 total revisions

  $ cd ..
