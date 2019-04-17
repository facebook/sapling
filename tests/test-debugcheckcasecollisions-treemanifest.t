The ordering and format of case collisions detected using treemanifest is
different, so this is a different test script.

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF

  $ hgcloneshallow ssh://user@dummy/master client -q
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=
  > treemanifest=
  > [remotefilelog]
  > usefastdatapack=True
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > EOF

  $ sorted() {
  >   "$@" > $TESTTMP/out
  >   local rc=$?
  >   sort < $TESTTMP/out
  >   rm -f $TESTTMP/out
  >   return $rc
  > }

  $ mkdir -p dirA/subdirA dirA/subdirB dirB
  $ touch dirA/subdirA/file1 dirA/subdirA/File10 dirA/subdirB/file2 dirB/file3 file4
  $ hg commit -Aqm "base"

Check basic case collisions
  $ sorted hg debugcheckcasecollisions DIRA/subdira/FILE1 DIRA/SUBDIRB/file2 DIRB/FILE3
  DIRA (directory for DIRA/SUBDIRB/file2) conflicts with dirA
  DIRB (directory for DIRB/FILE3) conflicts with dirB
  [1]
  $ sorted hg debugcheckcasecollisions dirA/subdirA/FILE1 dirA/SUBDIRB/file2 dirB/FILE3
  dirA/SUBDIRB (directory for dirA/SUBDIRB/file2) conflicts with dirA/subdirB
  dirA/subdirA/FILE1 conflicts with dirA/subdirA/file1
  dirB/FILE3 conflicts with dirB/file3
  [1]

Check a dir that collides with a file
  $ hg debugcheckcasecollisions FILE4/foo
  FILE4 (directory for FILE4/foo) conflicts with file4
  [1]

Check a file that collides with a dir
  $ hg debugcheckcasecollisions DIRb
  DIRb conflicts with dirB
  [1]

Check self-conflicts
  $ hg debugcheckcasecollisions newdir/newfile NEWdir/newfile newdir/NEWFILE
  NEWdir/newfile conflicts with newdir/newfile
  NEWdir (directory for NEWdir/newfile) conflicts with newdir (directory for newdir/newfile)
  newdir/NEWFILE conflicts with newdir/newfile
  [1]

Check against a particular revision
  $ hg debugcheckcasecollisions -r 0 FILE4
  FILE4 conflicts with file4
  [1]

#if no-icasefs
Check case collision on top of the commit which introduces it
(this is how this command is used: it runs from the hook on top of the commit
being checked, and it gets passed a list of file adds)
  $ touch dirA/subdirA/file10
  $ hg commit -Aqm new
  $ hg debugcheckcasecollisions dirA/subdirA/file10
  dirA/subdirA/file10 conflicts with dirA/subdirA/File10
  [1]
#endif
