  $ mkdir test
  $ cd test
  $ echo foo>foo
  $ hg init
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

