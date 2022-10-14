#chg-compatible
#debugruntest-compatible

#if no-windows no-osx

  $ setconfig config.use-rust=true
  $ mkdir -p xdgconf/sapling
  $ echo '[ui]' > xdgconf/sapling/sapling.conf
  $ echo 'username = foobar' >> xdgconf/sapling/sapling.conf
  $ XDG_CONFIG_HOME="`pwd`/xdgconf" ; export XDG_CONFIG_HOME
  $ unset HGRCPATH
  $ hg config ui.username 2>/dev/null
  foobar

  $ mkdir -p home/.config/sapling
  $ echo '[ui]' > home/.config/sapling/sapling.conf
  $ echo 'username = bazbaz' >> home/.config/sapling/sapling.conf
  $ HOME="`pwd`/home" ; export HOME
  $ unset XDG_CONFIG_HOME
  $ hg config ui.username 2>/dev/null
  bazbaz

#endif
