  $ hg init t
  $ cd t

  $ echo 1 > a
  $ hg ci -qAm "first"

  $ hg cp a b
  $ hg mv a c
  $ echo 2 >> b
  $ echo 2 >> c

  $ hg ci -qAm "second"

  $ hg co -C 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

  $ echo 0 > a
  $ echo 1 >> a

  $ hg ci -qAm "other"

  $ hg merge --debug
    searching for copies back to rev 1
    unmatched files in other:
     b
     c
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
     src: 'a' -> dst: 'c' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: b8bf91eeebbc, local: add3f11052fa+, remote: 17c05bb7fcb6
   preserving a for resolve of b
   preserving a for resolve of c
  removing a
  starting 4 threads for background file closing (?)
   b: remote moved from a -> m (premerge)
  picked tool ':merge' for b (binary False symlink False changedelete False)
  merging a and b to b
  my b@add3f11052fa+ other b@17c05bb7fcb6 ancestor a@b8bf91eeebbc
   premerge successful
   c: remote moved from a -> m (premerge)
  picked tool ':merge' for c (binary False symlink False changedelete False)
  merging a and c to c
  my c@add3f11052fa+ other c@17c05bb7fcb6 ancestor a@b8bf91eeebbc
   premerge successful
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

file b
  $ cat b
  0
  1
  2

file c
  $ cat c
  0
  1
  2

Test disabling copy tracing

- first verify copy metadata was kept

  $ hg up -qC 2
  $ hg rebase --keep -d 1 -b 2 --config extensions.rebase=
  rebasing 2:add3f11052fa "other" (tip)
  merging b and a to b
  merging c and a to c

  $ cat b
  0
  1
  2

- next verify copy metadata is lost when disabled

  $ hg strip -r . --config extensions.strip=
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/t/.hg/strip-backup/550bd84c0cd3-fc575957-backup.hg (glob)
  $ hg up -qC 2
  $ hg rebase --keep -d 1 -b 2 --config extensions.rebase= --config experimental.disablecopytrace=True --config ui.interactive=True << EOF
  > c
  > EOF
  rebasing 2:add3f11052fa "other" (tip)
  remote changed a which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? c

  $ cat b
  1
  2

  $ cd ..

Verify disabling copy tracing still keeps copies from rebase source

  $ hg init copydisable
  $ cd copydisable
  $ touch a
  $ hg ci -Aqm 'add a'
  $ touch b
  $ hg ci -Aqm 'add b, c'
  $ hg cp b x
  $ echo x >> x
  $ hg ci -qm 'copy b->x'
  $ hg up -q 1
  $ touch z
  $ hg ci -Aqm 'add z'
  $ hg log -G -T '{rev} {desc}\n'
  @  3 add z
  |
  | o  2 copy b->x
  |/
  o  1 add b, c
  |
  o  0 add a
  
  $ hg rebase -d . -b 2 --config extensions.rebase= --config experimental.disablecopytrace=True
  rebasing 2:6adcf8c12e7d "copy b->x"
  saved backup bundle to $TESTTMP/copydisable/.hg/strip-backup/6adcf8c12e7d-ce4b3e75-backup.hg (glob)
  $ hg up -q 3
  $ hg log -f x -T '{rev} {desc}\n'
  3 copy b->x
  1 add b, c

  $ cd ../

Verify we duplicate existing copies, instead of detecting them

  $ hg init copydisable3
  $ cd copydisable3
  $ touch a
  $ hg ci -Aqm 'add a'
  $ hg cp a b
  $ hg ci -Aqm 'copy a->b'
  $ hg mv b c
  $ hg ci -Aqm 'move b->c'
  $ hg up -q 0
  $ hg cp a b
  $ echo b >> b
  $ hg ci -Aqm 'copy a->b (2)'
  $ hg log -G -T '{rev} {desc}\n'
  @  3 copy a->b (2)
  |
  | o  2 move b->c
  | |
  | o  1 copy a->b
  |/
  o  0 add a
  
  $ hg rebase -d 2 -s 3 --config extensions.rebase= --config experimental.disablecopytrace=True
  rebasing 3:47e1a9e6273b "copy a->b (2)" (tip)
  saved backup bundle to $TESTTMP/copydisable3/.hg/strip-backup/47e1a9e6273b-2d099c59-backup.hg (glob)

  $ hg log -G -f b
  @  changeset:   3:76024fb4b05b
  :  tag:         tip
  :  user:        test
  :  date:        Thu Jan 01 00:00:00 1970 +0000
  :  summary:     copy a->b (2)
  :
  o  changeset:   0:ac82d8b1f7c4
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  
