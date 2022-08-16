#chg-compatible
#debugruntest-compatible
  $ configure modern
  $ newrepo

Make sure we do not rewrite by default:
  $ cat .hg/reponame
  reponame-default (no-eol)
  $ LOG=configparser::hg=debug hg log 2>&1 | grep written
  [1]

Rewrite on wrong reponame:
  $ echo foobar > .hg/reponame
  $ LOG=configparser::hg=debug hg log  2>&1 | grep written
  DEBUG configparser::hg: repo name: written to .hg/reponame
  $ cat .hg/reponame
  reponame-default (no-eol)
