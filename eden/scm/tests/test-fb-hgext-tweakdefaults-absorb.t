#debugruntest-compatible
#chg-compatible

  $ enable tweakdefaults
  $ enable absorb

Commit date defaults based on tweakdefaults
  $ newrepo
  $ echo foo > a
  $ hg ci -m 'a' -A a
  $ echo bar >> a
  $ hg absorb -qa
  $ hg log -r . -T '{date}\n'
  1657671627.00

Don't default when absorbkeepdate is set
  $ newrepo
  $ echo foo > a
  $ hg ci -m 'a' -A a
  $ echo bar >> a
  $ hg absorb -qa --config tweakdefaults.absorbkeepdate=true
  $ hg log -r . -T '{date}\n'
  0.00

