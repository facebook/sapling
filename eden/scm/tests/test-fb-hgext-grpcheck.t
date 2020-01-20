#chg-compatible

  $ enable grpcheck

mock os.getgroups and grp.getgrnam

  $ newext mockgrp <<EOF
  > import os, grp
  > def _getgroups():
  >     return map(int, os.environ.get('HGMOCKGRPS', '1000').split())
  > class _grp(object):
  >     def __init__(self, gid):
  >         self.gr_gid = gid
  > def _getgrnam(name):
  >     if name == 'users':
  >         gid = 1000
  >     elif name == 'devs':
  >         gid = 2000
  >     else:
  >         raise KeyError()
  >     return _grp(gid)
  > os.getgroups = _getgroups
  > grp.getgrnam = _getgrnam
  > EOF

  $ readconfig <<EOF
  > [grpcheck]
  > groups = users, devs
  > warning = You should be in %s group.
  > overrides.chgserver.idletimeout = 3
  > overrides.ui.foo = bar
  > EOF

when the user is not in those groups, warnings are printed

  $ newrepo
  You should be in devs group.
  $ hg log
  You should be in devs group.
  $ HGMOCKGRPS=100 hg log
  You should be in users group.
  $ HGMOCKGRPS='100 1000' hg log
  You should be in devs group.

customized warning message

  $ hg --config grpcheck.warning=foo log
  foo

no warning if HGPLAIN or grpcheck.warning is empty

  $ hg --config grpcheck.warning= log
  $ HGPLAIN=1 hg log

warning does not affect write action (commit)

  $ touch a
  $ hg commit -A a -m a
  You should be in devs group.
  $ hg log -T '{rev} {node}\n'
  You should be in devs group.
  0 3903775176ed42b1458a6281db4a0ccf4d9f287a

config overrides

  $ HGPLAIN=1 hg config --untrusted chgserver.idletimeout
  3
  $ cd .. # use repo ui
  $ HGPLAIN=1 hg config --untrusted ui.foo
  bar
