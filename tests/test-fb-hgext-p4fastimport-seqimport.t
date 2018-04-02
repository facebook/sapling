#require p4

  $ . $TESTDIR/p4setup.sh
  $ cat >> $HGRCPATH<<EOF
  > [p4fastimport]
  > metadata=metadata.sql
  > EOF
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
  $ echo modified >> Main/amove
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
  $ hg p4seqimport --debug -P $P4ROOT $P4CLIENT --limit 1
  loading changelist numbers.
  2 changelists to import.
  importing 1 only because of --limit.
  importing CL1
  adding Main/a
  adding Main/b
  committing files:
  Main/a
  Main/b
  committing manifest
  committing changelog
  updating the branch cache
  writing metadata to sqlite
  $ hg p4seqimport --debug -P $P4ROOT $P4CLIENT --limit 50
  incremental import from changelist: 2, node: * (glob)
  loading changelist numbers.
  1 changelists to import.
  importing CL2
  adding Main/c
  copying Main/a to Main/amove
  removing Main/a
  committing files:
  Main/amove
   Main/amove: copy Main/a:* (glob)
  Main/b
  Main/c
  committing manifest
  committing changelog
  updating the branch cache
  writing metadata to sqlite
  $ hg log -T '{desc} CL={extras.p4changelist}\n'
  second CL=2
  first CL=1
  $ hg log -T "$HGLOGTEMPLATE"
  second
  ADD=Main/amove Main/c
  DEL=Main/a
  MOD=Main/b
  COP
  Main/a => Main/amove
  first
  ADD=Main/a Main/b
  DEL=
  MOD=
  COP
  $ hg debugindex Main/b
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0       3     -1       0 1e88685f5dde 000000000000 000000000000
       1         3       6     -1       1 57fe91e2a37a 1e88685f5dde 000000000000

Ensure Main/amove was moved and modified
  $ hg cat Main/amove
  a
  modified

Verify that metadata is populated in sqlite file
  $ sqlite3 metadata.sql "SELECT * FROM revision_mapping"
  1|1|* (glob)
  2|2|* (glob)

End Test
  stopping the p4 server
