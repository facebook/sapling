#require p4

  $ . $TESTDIR/p4setup.sh
  $ cat >> $HGRCPATH<<EOF
  > [extensions]
  > lfs=
  > [lfs]
  > threshold=16
  > [p4fastimport]
  > lfsmetadata=metadata.sql
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

Add a largefile
  $ echo thisisalargefile! > Main/largefile
  $ p4 add Main/largefile
  //depot/Main/largefile#1 - opened for add
  $ p4 submit -d third
  Submitting change 3.
  Locking 1 files ...
  add //depot/Main/largefile#1
  Change 3 submitted.

Run seqimport limiting to one changelist
  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport --debug -P $P4ROOT $P4CLIENT --limit 1
  loading changelist numbers.
  3 changelists to import.
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
  calling hook commit.lfs: hgext.lfs.checkrequireslfs
  writing metadata to sqlite

Run seqimport again for up to 50 changelists
  $ hg p4seqimport --debug -P $P4ROOT $P4CLIENT --limit 50
  incremental import from changelist: 2, node: * (glob)
  loading changelist numbers.
  2 changelists to import.
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
  calling hook commit.lfs: hgext.lfs.checkrequireslfs
  writing metadata to sqlite
  importing CL3
  adding Main/largefile
  committing files:
  Main/largefile
  committing manifest
  committing changelog
  updating the branch cache
  largefile: Main/largefile, oid: 3c2631136e12ba309517e289322ea95ccc93a30d04265e7ea1fdf643fe59ed07
  calling hook commit.lfs: hgext.lfs.checkrequireslfs
  writing lfs metadata to sqlite
  writing metadata to sqlite

Confirm p4changelist is in commit extras
  $ hg log -T '{desc} CL={extras.p4changelist}\n'
  third CL=3
  second CL=2
  first CL=1
  $ hg log -T "$HGLOGTEMPLATE"
  third
  ADD=Main/largefile
  DEL=
  MOD=
  COP
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
  3|3|* (glob)
  $ sqlite3 metadata.sql "SELECT id, cl, path FROM p4_lfs_map"
  1|3|//depot/Main/largefile

End Test
  stopping the p4 server
