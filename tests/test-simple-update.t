  $ hg init test
  $ cd test
  $ echo foo>foo
  $ hg addremove
  adding foo
  $ hg commit -m "1"

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

  $ hg clone . ../branch
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../branch
  $ hg co
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo bar>>foo
  $ hg commit -m "2"

  $ cd ../test

  $ hg pull ../branch
  pulling from ../branch
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 30aff43faee1
  (run 'hg update' to get a working copy)

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 2 changesets, 2 total revisions

  $ hg co
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat foo
  foo
  bar

  $ hg manifest --debug
  6f4310b00b9a147241b071a60c28a650827fb03d 644   foo

update to rev 0 with a date

  $ hg upd -d foo 0
  abort: you can't specify a revision and a date
  [255]

  $ cd ..

update with worker processes

#if no-windows

  $ cat <<EOF > forceworker.py
  > from mercurial import extensions, worker
  > def nocost(orig, ui, costperop, nops):
  >     return worker._numworkers(ui) > 1
  > def uisetup(ui):
  >     extensions.wrapfunction(worker, 'worthwhile', nocost)
  > EOF

  $ hg init worker
  $ cd worker
  $ cat <<EOF >> .hg/hgrc
  > [extensions]
  > forceworker = $TESTTMP/forceworker.py
  > [worker]
  > numcpus = 4
  > EOF
  $ for i in `$PYTHON $TESTDIR/seq.py 1 100`; do
  >   echo $i > $i
  > done
  $ hg ci -qAm 'add 100 files'

  $ hg update null
  0 files updated, 0 files merged, 100 files removed, 0 files unresolved
  $ hg update -v | grep 100
  getting 100
  100 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ..

#endif
