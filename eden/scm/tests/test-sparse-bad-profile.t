#debugruntest-compatible

#require no-eden


  $ configure modernclient
  $ enable sparse
  $ newclientrepo

Make sure things work with invalid sparse profile:
  $ mkdir foo
  $ echo bar > foo/bar
  $ hg commit -Aqm foo
  $ echo "%include foo/" > .hg/sparse
  $ hg status
