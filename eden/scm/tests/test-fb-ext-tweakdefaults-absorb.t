#debugruntest-compatible
#chg-compatible

  $ enable tweakdefaults
  $ enable absorb

Commit date defaults to "now" based on tweakdefaults
  $ newrepo
  $ echo foo > a
  $ hg ci -m 'a' -A a
  $ echo bar >> a
  $ hg absorb -qa
  $ hg log -r . -T '{desc}\n' -d "yesterday to today"
  a

Don't default when absorbkeepdate is set
  $ newrepo
  $ echo foo > a
  $ hg ci -m 'a' -A a
  $ echo bar >> a
  $ hg absorb -qa --config tweakdefaults.absorbkeepdate=true
  $ hg log -r . -T '{desc} {date}\n'
  a 0.00

