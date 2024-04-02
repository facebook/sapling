#debugruntest-compatible

#require no-eden


  $ eagerepo
  $ enable absorb

Commit date defaults to "now" based on tweakdefaults
  $ newrepo
  $ echo foo > a
  $ hg ci -m 'a' -A a
  $ hg log -r . -T '{date}\n'
  0.00
  $ echo bar >> a
  $ hg absorb -qa --config devel.default-date='1 1'
  $ hg log -r . -T '{date}\n'
  1.01

Don't default when absorbkeepdate is set
  $ newrepo
  $ echo foo > a
  $ hg ci -m 'a' -A a
  $ echo bar >> a
  $ hg absorb -qa --config tweakdefaults.absorbkeepdate=true
  $ hg log -r . -T '{desc} {date}\n'
  a 0.00

