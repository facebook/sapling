#require p4

  $ . $TESTDIR/p4setup.sh

create extension

  $ cat > $TESTTMP/fail.py << EOF
  > from __future__ import absolute_import
  > import sys
  > from mercurial import (
  >     extensions,
  >     filelog,
  > )
  > tries = 0
  > def reposetup(ui, repo):
  >    def fail(orig, *args, **kwargs):
  >        global tries
  >        if tries == 1:
  >            raise Exception()
  >        tries += 1
  >        return orig(*args, **kwargs)
  >    e = extensions.find('p4fastimport')
  >    extensions.wrapfunction(filelog.filelog, 'add', fail)
  > EOF

populate the depot
  $ mkdir Main
  $ mkdir Main/b
  $ echo a > Main/a
  $ echo c > Main/b/c
  $ p4 add Main/a Main/b/c
  //depot/Main/a#1 - opened for add
  //depot/Main/b/c#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 2 files ...
  add //depot/Main/a#1
  add //depot/Main/b/c#1
  Change 1 submitted.

  $ p4 edit Main/a Main/b/c
  //depot/Main/a#1 - opened for edit
  //depot/Main/b/c#1 - opened for edit
  $ echo a >> Main/a
  $ echo c >> Main/b/c
  $ p4 submit -d second
  Submitting change 2.
  Locking 2 files ...
  edit //depot/Main/a#2
  edit //depot/Main/b/c#2
  Change 2 submitted.

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --bookmark master --verbose -P $P4ROOT hg-p4-import
  loading changelist numbers.
  2 changelists to import.
  loading list of files.
  2 files to import.
  importing repository.
  writing bookmark
  2 revision(s), 2 file(s) imported.

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 2 changesets, 4 total revisions

Enable extensions that will test transaction recovery

  $ echo "[p4fastimport]" >> $HGRCPATH
  $ echo "useworker=force" >> $HGRCPATH

One more submit

  $ cd $p4wd
  $ p4 edit Main/b/c
  //depot/Main/b/c#2 - opened for edit
  $ echo c >> Main/b/c
  $ p4 submit -d third
  Submitting change 3.
  Locking 1 files ...
  edit //depot/Main/b/c#3
  Change 3 submitted.
  $ p4 edit Main/b/c
  //depot/Main/b/c#3 - opened for edit
  $ echo c >> Main/b/c
  $ p4 submit -d foruth
  Submitting change 4.
  Locking 1 files ...
  edit //depot/Main/b/c#4
  Change 4 submitted.

  $ cd $hgwd
  $ hg --config "extensions.fail=$TESTTMP/fail.py" p4fastimport --bookmark master -P $P4ROOT hg-p4-import 2>&1 | grep -E "^transaction|^rollback"
  transaction abort!
  rollback complete

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 2 changesets, 4 total revisions

End Test

  stopping the p4 server
