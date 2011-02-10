  $ mkdir test
  $ cd test

  $ echo foo>foo
  $ hg init
  $ hg addremove
  adding foo
  $ hg commit -m 1

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

  $ hg serve -p $HGPORT -d --pid-file=hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..

  $ hg clone --pull http://foo:bar@localhost:$HGPORT/ copy
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd copy
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

  $ hg co
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat foo
  foo

  $ hg manifest --debug
  2ed2a3912a0b24502043eae84ee4b279c18b90dd 644   foo

  $ hg pull
  pulling from http://foo:***@localhost:$HGPORT/
  searching for changes
  no changes found

  $ hg rollback --dry-run --verbose
  repository tip rolled back to revision -1 (undo pull: http://foo:***@localhost:$HGPORT/)

Issue622: hg init && hg pull -u URL doesn't checkout default branch

  $ cd ..
  $ hg init empty
  $ cd empty
  $ hg pull -u ../test
  pulling from ../test
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test 'file:' uri handling:

  $ hg pull -q file://../test-doesnt-exist
  abort: repository /test-doesnt-exist not found!
  [255]

  $ hg pull -q file:../test

It's tricky to make file:// URLs working on every platform with
regular shell commands.

  $ URL=`python -c "import os; print 'file://foobar' + ('/' + os.getcwd().replace(os.sep, '/')).replace('//', '/') + '/../test'"`
  $ hg pull -q "$URL"

