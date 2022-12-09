#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig experimental.allowfilepeer=True

Issue586: removing remote files after merge appears to corrupt the
dirstate

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg ci -Ama
  adding a

  $ hg init ../b
  $ cd ../b
  $ echo b > b
  $ hg ci -Amb
  adding b

  $ hg pull -f ../a
  pulling from ../a
  searching for changes
  warning: repository is unrelated
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg rm -f a
  $ hg ci -Amc

  $ hg st -A
  C b
  $ cd ..

Issue1433: Traceback after two unrelated pull, two move, a merge and
a commit (related to issue586)

create test repos

  $ hg init repoa
  $ touch repoa/a
  $ hg -R repoa ci -Am adda
  adding a

  $ hg init repob
  $ touch repob/b
  $ hg -R repob ci -Am addb
  adding b

  $ hg init repoc
  $ cd repoc
  $ hg pull ../repoa
  pulling from ../repoa
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  $ hg goto
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir tst
  $ hg mv * tst
  $ hg ci -m "import a in tst"
  $ hg pull -f ../repob
  pulling from ../repob
  searching for changes
  warning: repository is unrelated
  requesting all changes
  adding changesets
  adding manifests
  adding file changes

merge both repos

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ mkdir src

move b content

  $ hg mv b src
  $ hg ci -m "import b in src"
  $ hg manifest
  src/b
  tst/a

  $ cd ..
