#chg-compatible
#debugruntest-compatible
  $ configure modern
  $ newrepo

Make sure we do not rewrite by default:
  $ cat .hg/reponame
  reponame-default (no-eol)
  $ LOG=configloader::hg=debug hg log 2>&1 | grep written
  [1]

Rewrite on wrong reponame:
  $ echo foobar > .hg/reponame
  $ LOG=configloader::hg=debug hg log  2>&1 | grep written
  DEBUG configloader::hg: repo name: written to reponame file
  $ cat .hg/reponame
  reponame-default (no-eol)
