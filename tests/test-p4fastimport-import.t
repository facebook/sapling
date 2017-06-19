#require p4

  $ . $TESTDIR/p4setup.sh

populate the depot
  $ mkdir Main
  $ mkdir Main/b
  $ echo a > Main/a
  $ echo c > Main/b/c
  $ echo d > Main/d
  $ p4 add Main/a Main/b/c Main/d
  //depot/Main/a#1 - opened for add
  //depot/Main/b/c#1 - opened for add
  //depot/Main/d#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 3 files ...
  add //depot/Main/a#1
  add //depot/Main/b/c#1
  add //depot/Main/d#1
  Change 1 submitted.

  $ p4 edit Main/a Main/b/c Main/d
  //depot/Main/a#1 - opened for edit
  //depot/Main/b/c#1 - opened for edit
  //depot/Main/d#1 - opened for edit
  $ echo a >> Main/a
  $ echo c >> Main/b/c
  $ echo d >> Main/d
  $ p4 submit -d second
  Submitting change 2.
  Locking 3 files ...
  edit //depot/Main/a#2
  edit //depot/Main/b/c#2
  edit //depot/Main/d#2
  Change 2 submitted.

Test archiving something

  $ cat >desc <<EOF
  > Depot: archive
  > Description: Archive
  > Type: archive
  > Map: archive/...
  > EOF
  $ p4 depot -i <desc
  Depot archive saved.
  $ p4 archive -t -D archive //depot/Main/d
  Archiving //depot/Main/d#2 to //archive/depot/Main/d.
  Archiving //depot/Main/d#1 to //archive/depot/Main/d.

Test keyword extension
  $ cat >test.c <<EOF
  > \$Id\$
  > \$Header\$
  > \$Date\$
  > \$DateTime\$
  > \$Change\$
  > \$File\$
  > \$Revision\$
  > \$Author\$
  > EOF
  $ p4 add test.c
  //depot/test.c#1 - opened for add
  $ p4 submit -d before_expand
  Submitting change 3.
  Locking 1 files ...
  add //depot/test.c#1
  Change 3 submitted.

  $ p4 edit -t +k test.c
  //depot/test.c#1 - opened for edit
  $ p4 submit -d after_expand
  Submitting change 4.
  Locking 1 files ...
  edit //depot/test.c#2
  Change 4 submitted.
  //depot/test.c#2 - refreshing
  $ p4 files test.c
  //depot/test.c#2 - edit change 4 (ktext)
  $ p4 print -q //depot/test.c#2
  $Id: //depot/test.c#2 $
  $Header: //depot/test.c#2 $
  $Date: * $ (glob)
  $DateTime: * $ (glob)
  $Change: 4 $
  $File: //depot/test.c $
  $Revision: #2 $
  $Author: * $ (glob)

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --bookmark master --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  4 changelists to import.
  loading list of files.
  3 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  writing filelog: a80d06849b33, p1 b789fdd96dc2, linkrev 1, 4 bytes, src: *, path: Main/a (glob)
  writing filelog: 149da44f2a4e, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/b/c (glob)
  writing filelog: b11e10a88bfa, p1 149da44f2a4e, linkrev 1, 4 bytes, src: *, path: Main/b/c (glob)
  writing filelog: 7083c74fbb1d, p1 000000000000, linkrev 2, 68 bytes, src: *, path: test.c (glob)
  writing filelog: 67de97119b9e, p1 7083c74fbb1d, linkrev 3, 68 bytes, src: *, path: test.c (glob)
  changelist 1: writing manifest. node: a9ab65129a6d p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: aff99eae550e p1: a9ab65129a6d p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second
  changelist 3: writing manifest. node: 25805ee52828 p1: aff99eae550e p2: 000000000000 linkrev: 2
  changelist 3: writing changelog: before_expand
  changelist 4: writing manifest. node: 6bd331e4b9db p1: 25805ee52828 p2: 000000000000 linkrev: 3
  changelist 4: writing changelog: after_expand
  writing bookmark
  updating the branch cache
  4 revision(s), 3 file(s) imported.
  $ hg cat -r tip test.c
  $Id$
  $Header$
  $Date$
  $DateTime$
  $Change$
  $File$
  $Revision$
  $Author$

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 4 changesets, 6 total revisions

  $ hg update master
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master)

  $ hg manifest -r master
  Main/a
  Main/b/c
  test.c

End Test

  stopping the p4 server
