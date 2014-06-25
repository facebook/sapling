  $ hg init repo
  $ cd repo
  $ echo 123 > a
  $ echo 123 > c
  $ echo 123 > e
  $ hg add a c e
  $ hg commit -m "first" a c e

nothing changed

  $ hg revert
  abort: no files or directories specified
  (use --all to revert all files)
  [255]
  $ hg revert --all

Introduce some changes and revert them
--------------------------------------

  $ echo 123 > b

should show b unknown

  $ hg status
  ? b
  $ echo 12 > c

should show b unknown and c modified

  $ hg status
  M c
  ? b
  $ hg add b

should show b added and c modified

  $ hg status
  M c
  A b
  $ hg rm a

should show a removed, b added and c modified

  $ hg status
  M c
  A b
  R a

revert removal of a file

  $ hg revert a

should show b added, copy saved, and c modified

  $ hg status
  M c
  A b

revert addition of a file

  $ hg revert b

should show b unknown, and c modified

  $ hg status
  M c
  ? b

revert modification of a file (--no-backup)

  $ hg revert --no-backup c

should show unknown: b

  $ hg status
  ? b

revert deletion (! status) of a added file
------------------------------------------

  $ hg add b

should show b added

  $ hg status b
  A b
  $ rm b

should show b deleted

  $ hg status b
  ! b
  $ hg revert -v b
  forgetting b

should not find b

  $ hg status b
  b: * (glob)

should show a c e

  $ ls
  a
  c
  e

Test creation of backup (.orig) files
-------------------------------------

  $ echo z > e
  $ hg revert --all -v
  saving current version of e as e.orig
  reverting e

revert on clean file (no change)
--------------------------------

  $ hg revert a
  no changes needed to a

revert on an untracked file
---------------------------

  $ echo q > q
  $ hg revert q
  file not managed: q
  $ rm q

revert on file that does not exists
-----------------------------------

  $ hg revert notfound
  notfound: no such file in rev 334a9e57682c
  $ touch d
  $ hg add d
  $ hg rm a
  $ hg commit -m "second"
  $ echo z > z
  $ hg add z
  $ hg st
  A z
  ? e.orig

revert to another revision (--rev)
----------------------------------

  $ hg revert --all -r0
  adding a
  removing d
  forgetting z

revert explicitly to parent (--rev)
-----------------------------------

  $ hg revert --all -rtip
  forgetting a
  undeleting d
  $ rm a *.orig

revert to another revision (--rev) and exact match
--------------------------------------------------

exact match are more silent

  $ hg revert -r0 a
  $ hg st a
  A a
  $ hg rm d
  $ hg st d
  R d

should silently keep d removed

  $ hg revert -r0 d
  $ hg st d
  R d

  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

revert of exec bit
------------------

#if execbit
  $ chmod +x c
  $ hg revert --all
  reverting c

should print non-executable

  $ test -x c || echo non-executable
  non-executable

  $ chmod +x c
  $ hg commit -m exe

  $ chmod -x c
  $ hg revert --all
  reverting c

should print executable

  $ test -x c && echo executable
  executable
#endif

  $ cd ..


Issue241: update and revert produces inconsistent repositories
--------------------------------------------------------------

  $ hg init a
  $ cd a
  $ echo a >> a
  $ hg commit -A -d '1 0' -m a
  adding a
  $ echo a >> a
  $ hg commit -d '2 0' -m a
  $ hg update 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir b
  $ echo b > b/b

call `hg revert` with no file specified
---------------------------------------

  $ hg revert -rtip
  abort: no files or directories specified
  (use --all to revert all files, or 'hg update 1' to update)
  [255]

call `hg revert` with --all
---------------------------

  $ hg revert --all -rtip
  reverting a


Issue332: confusing message when reverting directory
----------------------------------------------------

  $ hg ci -A -m b
  adding b/b
  created new head
  $ echo foobar > b/b
  $ mkdir newdir
  $ echo foo > newdir/newfile
  $ hg add newdir/newfile
  $ hg revert b newdir
  reverting b/b (glob)
  forgetting newdir/newfile (glob)
  $ echo foobar > b/b
  $ hg revert .
  reverting b/b (glob)


reverting a rename target should revert the source
--------------------------------------------------

  $ hg mv a newa
  $ hg revert newa
  $ hg st a newa
  ? newa

  $ cd ..

  $ hg init ignored
  $ cd ignored
  $ echo '^ignored$' > .hgignore
  $ echo '^ignoreddir$' >> .hgignore
  $ echo '^removed$' >> .hgignore

  $ mkdir ignoreddir
  $ touch ignoreddir/file
  $ touch ignoreddir/removed
  $ touch ignored
  $ touch removed

4 ignored files (we will add/commit everything)

  $ hg st -A -X .hgignore
  I ignored
  I ignoreddir/file
  I ignoreddir/removed
  I removed
  $ hg ci -qAm 'add files' ignored ignoreddir/file ignoreddir/removed removed

  $ echo >> ignored
  $ echo >> ignoreddir/file
  $ hg rm removed ignoreddir/removed

should revert ignored* and undelete *removed
--------------------------------------------

  $ hg revert -a --no-backup
  reverting ignored
  reverting ignoreddir/file (glob)
  undeleting ignoreddir/removed (glob)
  undeleting removed
  $ hg st -mardi

  $ hg up -qC
  $ echo >> ignored
  $ hg rm removed

should silently revert the named files
--------------------------------------

  $ hg revert --no-backup ignored removed
  $ hg st -mardi

Reverting copy (issue3920)
--------------------------

someone set up us the copies

  $ rm .hgignore
  $ hg update -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg mv ignored allyour
  $ hg copy removed base
  $ hg commit -m rename

copies and renames, you have no chance to survive make your time (issue3920)

  $ hg update '.^'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg revert -rtip -a
  adding allyour
  adding base
  removing ignored
  $ hg status -C
  A allyour
    ignored
  A base
    removed
  R ignored

Test revert of a file added by one side of the merge
====================================================

remove any pending change

  $ hg revert --all
  forgetting allyour
  forgetting base
  undeleting ignored
  $ hg purge --all --config extensions.purge=

Adds a new commit

  $ echo foo > newadd
  $ hg add newadd
  $ hg commit -m 'other adds'
  created new head


merge it with the other head

  $ hg merge # merge 1 into 2
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg summary
  parent: 2:b8ec310b2d4e tip
   other adds
  parent: 1:f6180deb8fbe 
   rename
  branch: default
  commit: 2 modified, 1 removed (merge)
  update: (current)

clarifies who added what

  $ hg status
  M allyour
  M base
  R ignored
  $ hg status --change 'p1()'
  A newadd
  $ hg status --change 'p2()'
  A allyour
  A base
  R ignored

revert file added by p1() to p1() state
-----------------------------------------

  $ hg revert -r 'p1()' 'glob:newad?'
  $ hg status
  M allyour
  M base
  R ignored

revert file added by p1() to p2() state
------------------------------------------

  $ hg revert -r 'p2()' 'glob:newad?'
  removing newadd
  $ hg status
  M allyour
  M base
  R ignored
  R newadd

revert file added by p2() to p2() state
------------------------------------------

  $ hg revert -r 'p2()' 'glob:allyou?'
  $ hg status
  M allyour
  M base
  R ignored
  R newadd

revert file added by p2() to p1() state
------------------------------------------

  $ hg revert -r 'p1()' 'glob:allyou?'
  removing allyour
  $ hg status
  M base
  R allyour
  R ignored
  R newadd


