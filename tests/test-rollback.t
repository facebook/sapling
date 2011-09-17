setup repo
  $ hg init t
  $ cd t
  $ echo a > a
  $ hg commit -Am'add a'
  adding a
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
  $ hg parents
  changeset:   0:1f0dee641bb7
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  

rollback to null revision
  $ hg status
  $ hg rollback
  repository tip rolled back to revision -1 (undo commit)
  working directory now based on revision -1
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  0 files, 0 changesets, 0 total revisions
  $ hg parents
  $ hg status
  A a

Two changesets this time so we rollback to a real changeset
  $ hg commit -m'add a again'
  $ echo a >> a
  $ hg commit -m'modify a'

Test issue 902 (current branch is preserved)
  $ hg branch test
  marked working directory as branch test
  $ hg rollback
  repository tip rolled back to revision 0 (undo commit)
  working directory now based on revision 0
  $ hg branch
  default

Test issue 1635 (commit message saved)
  $ cat .hg/last-message.txt ; echo
  modify a

Test rollback of hg before issue 902 was fixed

  $ hg commit -m "test3"
  $ hg branch test
  marked working directory as branch test
  $ rm .hg/undo.branch
  $ hg rollback
  repository tip rolled back to revision 0 (undo commit)
  named branch could not be reset: current branch is still 'test'
  working directory now based on revision 0
  $ hg branch
  test

rollback by pretxncommit saves commit message (issue 1635)

  $ echo a >> a
  $ hg --config hooks.pretxncommit=false commit -m"precious commit message"
  transaction abort!
  rollback completed
  abort: pretxncommit hook exited with status * (glob)
  [255]
  $ cat .hg/last-message.txt ; echo
  precious commit message

same thing, but run $EDITOR

  $ cat > editor << '__EOF__'
  > #!/bin/sh
  > echo "another precious commit message" > "$1"
  > __EOF__
  $ chmod +x editor
  $ HGEDITOR="'`pwd`'"/editor hg --config hooks.pretxncommit=false commit 2>&1
  transaction abort!
  rollback completed
  note: commit message saved in .hg/last-message.txt
  abort: pretxncommit hook exited with status * (glob)
  [255]
  $ cat .hg/last-message.txt
  another precious commit message

test rollback on served repository

  $ hg commit -m "precious commit message"
  $ hg serve -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..
  $ hg clone http://localhost:$HGPORT u
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd u
  $ hg id default
  8902593132ae

now rollback and observe that 'hg serve' reloads the repository and
presents the correct tip changeset:

  $ hg -R ../t rollback
  repository tip rolled back to revision 0 (undo commit)
  working directory now based on revision 0
  $ hg id default
  23b0221f3370
