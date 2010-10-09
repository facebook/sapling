Use hgrc within $TESTTMP

  $ HGRCPATH=`pwd`/hgrc
  $ export HGRCPATH

Basic syntax error

  $ echo "invalid" > $HGRCPATH
  $ hg version
  hg: parse error at $TESTTMP/hgrc:1: invalid
  [255]
  $ echo "" > $HGRCPATH

Issue1199: Can't use '%' in hgrc (eg url encoded username)

  $ hg init "foo%bar"
  $ hg clone "foo%bar" foobar
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd foobar
  $ cat .hg/hgrc
  [paths]
  default = */foo%bar (glob)
  $ hg paths
  default = */foo%bar (glob)
  $ hg showconfig
  bundle.mainreporoot=*/foobar (glob)
  paths.default=*/foo%bar (glob)
  $ cd ..

issue1829: wrong indentation

  $ echo '[foo]' > $HGRCPATH
  $ echo '  x = y' >> $HGRCPATH
  $ hg version
  hg: parse error at $TESTTMP/hgrc:2:   x = y
  [255]

  $ python -c "print '[foo]\nbar = a\n b\n c \n  de\n fg \nbaz = bif cb \n'" \
  > > $HGRCPATH
  $ hg showconfig foo
  foo.bar=a\nb\nc\nde\nfg
  foo.baz=bif cb

  $ FAKEPATH=/path/to/nowhere
  $ export FAKEPATH
  $ echo '%include $FAKEPATH/no-such-file' > $HGRCPATH
  $ hg version
  hg: parse error at $TESTTMP/hgrc:1: cannot include /path/to/nowhere/no-such-file (No such file or directory)
  [255]
  $ unset FAKEPATH

username expansion

  $ olduser=$HGUSER
  $ unset HGUSER

  $ FAKEUSER='John Doe'
  $ export FAKEUSER
  $ echo '[ui]' > $HGRCPATH
  $ echo 'username = $FAKEUSER' >> $HGRCPATH

  $ hg init usertest
  $ cd usertest
  $ touch bar
  $ hg commit --addremove --quiet -m "added bar"
  $ hg log --template "{author}\n"
  John Doe
  $ cd ..

  $ hg showconfig
  ui.username=$FAKEUSER

  $ unset FAKEUSER
  $ HGUSER=$olduser
  $ export HGUSER

showconfig with multiple arguments

  $ echo "[alias]" > $HGRCPATH
  $ echo "log = log -g" >> $HGRCPATH
  $ echo "[defaults]" >> $HGRCPATH
  $ echo "identify = -n" >> $HGRCPATH
  $ hg showconfig alias defaults
  alias.log=log -g
  defaults.identify=-n
  $ hg showconfig alias defaults.identify
  abort: only one config item permitted
  [255]
  $ hg showconfig alias.log defaults.identify
  abort: only one config item permitted
  [255]

HGPLAIN

  $ cd ..
  $ p=`pwd`
  $ echo "[ui]" > $HGRCPATH
  $ echo "debug=true" >> $HGRCPATH
  $ echo "fallbackencoding=ASCII" >> $HGRCPATH
  $ echo "quiet=true" >> $HGRCPATH
  $ echo "slash=true" >> $HGRCPATH
  $ echo "traceback=true" >> $HGRCPATH
  $ echo "verbose=true" >> $HGRCPATH
  $ echo "style=~/.hgstyle" >> $HGRCPATH
  $ echo "logtemplate={node}" >> $HGRCPATH
  $ echo "[defaults]" >> $HGRCPATH
  $ echo "identify=-n" >> $HGRCPATH
  $ echo "[alias]" >> $HGRCPATH
  $ echo "log=log -g" >> $HGRCPATH

customized hgrc

  $ hg showconfig
  read config from: $TESTTMP/hgrc
  $TESTTMP/hgrc:13: alias.log=log -g
  $TESTTMP/hgrc:11: defaults.identify=-n
  $TESTTMP/hgrc:2: ui.debug=true
  $TESTTMP/hgrc:3: ui.fallbackencoding=ASCII
  $TESTTMP/hgrc:4: ui.quiet=true
  $TESTTMP/hgrc:5: ui.slash=true
  $TESTTMP/hgrc:6: ui.traceback=true
  $TESTTMP/hgrc:7: ui.verbose=true
  $TESTTMP/hgrc:8: ui.style=~/.hgstyle
  $TESTTMP/hgrc:9: ui.logtemplate={node}

plain hgrc

  $ HGPLAIN=; export HGPLAIN
  $ hg showconfig --config ui.traceback=True --debug
  read config from: $TESTTMP/hgrc
  none: ui.traceback=True
  none: ui.verbose=False
  none: ui.debug=True
  none: ui.quiet=False
