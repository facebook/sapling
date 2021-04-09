#chg-compatible

#if no-windows no-osx

  $ mkdir -p xdgconf/hg
  $ echo '[ui]' > xdgconf/hg/hgrc
  $ echo 'username = foobar' >> xdgconf/hg/hgrc
  $ XDG_CONFIG_HOME="`pwd`/xdgconf" ; export XDG_CONFIG_HOME
  $ unset HGRCPATH
  $ hg config ui.username 2>/dev/null
  foobar

  $ mkdir -p home/.config/hg
  $ echo '[ui]' > home/.config/hg/hgrc
  $ echo 'username = bazbaz' >> home/.config/hg/hgrc
  $ HOME="`pwd`/home" ; export HOME
  $ unset XDG_CONFIG_HOME
  $ hg config ui.username 2>/dev/null
  bazbaz


#endif
