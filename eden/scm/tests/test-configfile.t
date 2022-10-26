#debugruntest-compatible
#chg-compatible

#if no-windows

  $ mkdir repo
  $ cd repo
  $ hg init

Test errors
  $ hg configfile --user --local
  abort: must select at most one of --user, --local, or --system
  [255]
  $ hg --cwd ../ configfile --local
  abort: --local must be used inside a repo
  [255]

Test locating user config
  $ hg configfile
  User config path: $TESTTMP/.hgrc
  Repo config path: $TESTTMP/repo/.hg/hgrc
  System config path: /etc/mercurial/system.rc
  $ hg configfile --user
  $TESTTMP/.hgrc
  $ HGIDENTITY=sl hg configfile --user
  $TESTTMP/.config/sapling/sapling.conf
  $ touch $TESTTMP/.hgrc
  $ HGIDENTITY=sl hg configfile --user
  $TESTTMP/.hgrc

Test locating other configs
  $ hg configfile --local
  $TESTTMP/repo/.hg/hgrc
  $ hg configfile --system
  /etc/mercurial/system.rc

#endif
