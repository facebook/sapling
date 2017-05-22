#if no-windows no-osx

  $ mkdir -p xdgconf/hg
  $ echo '[ui]' > xdgconf/hg/hgrc
  $ echo 'username = foobar' >> xdgconf/hg/hgrc
  $ XDG_CONFIG_HOME="`pwd`/xdgconf" ; export XDG_CONFIG_HOME
  $ unset HGRCPATH
  $ hg config ui.username 2>/dev/null
  foobar

#endif
