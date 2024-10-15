#require no-eden

#inprocess-hg-incompatible

Issue586: removing remote files after merge appears to corrupt the
dirstate

  $ newserver a
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ hg book master

  $ cd
  $ hg clone -qU test:a b
  $ cd b
  $ echo b > b
  $ hg ci -Amb
  adding b

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
  $ hg -R repoa whereami
  7132ab4568acf5245eda3a818f5e927761e093bd

  $ hg init repob
  $ touch repob/b
  $ hg -R repob ci -Am addb
  adding b
  $ hg -R repob whereami
  5ddceb3496526eca9300ea4b56d384785a1e31ba

  $ hg init repoc
  $ cd repoc
  $ hg pull -fr 7132ab456 ssh://user@dummy/repoa
  pulling from ssh://user@dummy/repoa
  adding changesets
  adding manifests
  adding file changes
  $ hg goto tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir tst
  $ hg mv * tst
  $ hg ci -m "import a in tst"
  $ hg pull -fr 5ddceb349 ../repob
  pulling from ../repob
  searching for changes
  warning: repository is unrelated
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
