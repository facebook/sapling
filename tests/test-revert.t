  $ hg init repo
  $ cd repo
  $ echo 123 > a
  $ echo 123 > c
  $ echo 123 > e
  $ hg add a c e
  $ hg commit -m "first" a c e
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
  $ hg revert a

should show b added, copy saved, and c modified

  $ hg status
  M c
  A b
  $ hg revert b

should show b unknown, and c modified

  $ hg status
  M c
  ? b
  $ hg revert --no-backup c

should show unknown: b

  $ hg status
  ? b
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
  b: No such file or directory

should show a c e

  $ ls
  a
  c
  e

should verbosely save backup to e.orig

  $ echo z > e
  $ hg revert --all -v
  saving current version of e as e.orig
  reverting e

should say no changes needed

  $ hg revert a
  no changes needed to a

should say file not managed

  $ echo q > q
  $ hg revert q
  file not managed: q
  $ rm q

should say file not found

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

should add a, remove d, forget z

  $ hg revert --all -r0
  adding a
  removing d
  forgetting z

should forget a, undelete d

  $ hg revert --all -rtip
  forgetting a
  undeleting d
  $ rm a *.orig

should silently add a

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

  $ cd ..


Issue241: update and revert produces inconsistent repositories

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

should fail - no arguments

  $ hg revert -rtip
  abort: no files or directories specified; use --all to revert the whole repo
  [255]

should succeed

  $ hg revert --all -rtip
  reverting a


Issue332: confusing message when reverting directory

  $ hg ci -A -m b
  adding b/b
  created new head
  $ echo foobar > b/b
  $ mkdir newdir
  $ echo foo > newdir/newfile
  $ hg add newdir/newfile
  $ hg revert b newdir
  reverting b/b
  forgetting newdir/newfile
  $ echo foobar > b/b
  $ hg revert .
  reverting b/b


reverting a rename target should revert the source

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

  $ hg revert -a --no-backup
  reverting ignored
  reverting ignoreddir/file
  undeleting ignoreddir/removed
  undeleting removed
  $ hg st -mardi

  $ hg up -qC
  $ echo >> ignored
  $ hg rm removed

should silently revert the named files

  $ hg revert --no-backup ignored removed
  $ hg st -mardi
