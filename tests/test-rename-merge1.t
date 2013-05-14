  $ hg init

  $ echo "[merge]" >> .hg/hgrc
  $ echo "followcopies = 1" >> .hg/hgrc

  $ echo foo > a
  $ echo foo > a2
  $ hg add a a2
  $ hg ci -m "start"

  $ hg mv a b
  $ hg mv a2 b2
  $ hg ci -m "rename"

  $ hg co 0
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved

  $ echo blahblah > a
  $ echo blahblah > a2
  $ hg mv a2 c2
  $ hg ci -m "modify"
  created new head

  $ hg merge -y --debug
    searching for copies back to rev 1
    unmatched files in local:
     c2
    unmatched files in other:
     b
     b2
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
     src: 'a2' -> dst: 'b2' !
     src: 'a2' -> dst: 'c2' !
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: af1939970a1c, local: 044f8520aeeb+, remote: 85c198ef2f6c
   a: remote moved to b -> m
    preserving a for resolve of b
   a2: divergent renames -> dr
   b2: remote created -> g
  removing a
  getting b2
  updating: b2 1/3 files (33.33%)
  updating: a 2/3 files (66.67%)
  picked tool 'internal:merge' for b (binary False symlink False)
  merging a and b to b
  my b@044f8520aeeb+ other b@85c198ef2f6c ancestor a@af1939970a1c
   premerge successful
  updating: a2 3/3 files (100.00%)
  note: possible conflict - a2 was renamed multiple times to:
   c2
   b2
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg status -AC
  M b
    a
  M b2
  R a
  C c2

  $ cat b
  blahblah

  $ hg ci -m "merge"

  $ hg debugindex b
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0      67  .....       1 57eacc201a7f 000000000000 000000000000 (re)
       1        67      72  .....       3 4727ba907962 000000000000 57eacc201a7f (re)

  $ hg debugrename b
  b renamed from a:dd03b83622e78778b403775d0d074b9ac7387a66

This used to trigger a "divergent renames" warning, despite no renames

  $ hg cp b b3
  $ hg cp b b4
  $ hg ci -A -m 'copy b twice'
  $ hg up eb92d88a9712
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg up
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg rm b3 b4
  $ hg ci -m 'clean up a bit of our mess'

We'd rather not warn on divergent renames done in the same changeset (issue2113)

  $ hg cp b b3
  $ hg mv b b4
  $ hg ci -A -m 'divergent renames in same changeset'
  $ hg up c761c6948de0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg up
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved

Check for issue2642

  $ hg init t
  $ cd t

  $ echo c0 > f1
  $ hg ci -Aqm0

  $ hg up null -q
  $ echo c1 > f1 # backport
  $ hg ci -Aqm1
  $ hg mv f1 f2
  $ hg ci -qm2

  $ hg up 0 -q
  $ hg merge 1 -q --tool internal:local
  $ hg ci -qm3

  $ hg merge 2
  merging f1 and f2 to f2
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ cat f2
  c0

  $ cd ..

Check for issue2089

  $ hg init repo2089
  $ cd repo2089

  $ echo c0 > f1
  $ hg ci -Aqm0

  $ hg up null -q
  $ echo c1 > f1
  $ hg ci -Aqm1

  $ hg up 0 -q
  $ hg merge 1 -q --tool internal:local
  $ echo c2 > f1
  $ hg ci -qm2

  $ hg up 1 -q
  $ hg mv f1 f2
  $ hg ci -Aqm3

  $ hg up 2 -q
  $ hg merge 3
  merging f1 and f2 to f2
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ cat f2
  c2

  $ cd ..

Check for issue3074

  $ hg init repo3074
  $ cd repo3074
  $ echo foo > file
  $ hg add file
  $ hg commit -m "added file"
  $ hg mv file newfile
  $ hg commit -m "renamed file"
  $ hg update 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg rm file
  $ hg commit -m "deleted file"
  created new head
  $ hg merge --debug
    searching for copies back to rev 1
    unmatched files in other:
     newfile
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'file' -> dst: 'newfile' %
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 19d7f95df299, local: 0084274f6b67+, remote: 5d32493049f0
   file: rename and delete -> rd
   newfile: remote created -> g
  getting newfile
  updating: newfile 1/2 files (50.00%)
  updating: file 2/2 files (100.00%)
  note: possible conflict - file was deleted and renamed to:
   newfile
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status
  M newfile
  $ cd ..
