#debugruntest-compatible

#require no-eden


  $ eagerepo
  $ mkdir repo
  $ cd repo
  $ hg init
  $ export PROGRAMDATA="C:\\ProgramData\\Facebook\\Mercurial\\"
  $ export APPDATA="$TESTTMP\\AppData\\Roaming\\"

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
  System config path: $TESTTMP/hgrc
  $ hg configfile --user
  $TESTTMP/.hgrc
  $ sl configfile --user
  $TESTTMP/.config/sapling/sapling.conf (linux !)
  $TESTTMP/Library/Preferences/sapling/sapling.conf (osx !)
  $TESTTMP\AppData\Roaming\sapling\sapling.conf (windows !)
  $ touch $TESTTMP/.hgrc
  $ sl configfile --user
  $TESTTMP/.hgrc

Test locating other configs
  $ hg configfile --local
  $TESTTMP/repo/.hg/hgrc
  $ hg configfile --system
  $TESTTMP/hgrc

Test outside a repo
  $ cd
  $ hg configfile
  User config path: $TESTTMP/.hgrc
  System config path: $TESTTMP/hgrc
