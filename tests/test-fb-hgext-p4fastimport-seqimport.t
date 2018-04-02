#require p4

  $ . $TESTDIR/p4setup.sh
  $ T_HGADD='ADD={file_adds}\n'
  $ T_HGDEL='DEL={file_dels}\n'
  $ T_HGMOD='MOD={file_mods}\n'
  $ T_HGCOP='COP\n{file_copies % "{source} => {name}\n"}'
  $ HGLOGTEMPLATE="{desc}\n${T_HGADD}${T_HGDEL}${T_HGMOD}${T_HGCOP}"

Populate depot
  $ mkdir Main
  $ echo a > Main/a
  $ echo b > Main/b
  $ p4 add Main/a Main/b
  //depot/Main/a#1 - opened for add
  //depot/Main/b#1 - opened for add
  $ p4 submit -d first
  Submitting change 1.
  Locking 2 files ...
  add //depot/Main/a#1
  add //depot/Main/b#1
  Change 1 submitted.

  $ p4 edit Main/a Main/b
  //depot/Main/a#1 - opened for edit
  //depot/Main/b#1 - opened for edit
  $ p4 move Main/a Main/amove
  //depot/Main/amove#1 - moved from //depot/Main/a#1
  $ echo bb >> Main/b
  $ echo c >> Main/c
  $ p4 add Main/c
  //depot/Main/c#1 - opened for add
  $ p4 submit -d second
  Submitting change 2.
  Locking 4 files ...
  move/delete //depot/Main/a#2
  move/add //depot/Main/amove#1
  edit //depot/Main/b#2
  add //depot/Main/c#1
  Change 2 submitted.

Run seqimport
  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport --debug -P $P4ROOT $P4CLIENT
  loading changelist numbers.
  2 changelists to import.
  importing CL1
  adding Main/a
  adding Main/b
  committing files:
  Main/a
  Main/b
  committing manifest
  committing changelog
  updating the branch cache
  committed changeset 0:* (glob)
  importing CL2
  adding Main/amove
  adding Main/c
  removing Main/a
  committing files:
  Main/amove
  Main/b
  Main/c
  committing manifest
  committing changelog
  updating the branch cache
  committed changeset 1:* (glob)
  $ hg log -T "$HGLOGTEMPLATE"
  second
  ADD=Main/amove Main/c
  DEL=Main/a
  MOD=Main/b
  COP
  first
  ADD=Main/a Main/b
  DEL=
  MOD=
  COP
  $ hg debugindex Main/b
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0       3     -1       0 1e88685f5dde 000000000000 000000000000
       1         3       6     -1       1 57fe91e2a37a 1e88685f5dde 000000000000

End Test
  stopping the p4 server
