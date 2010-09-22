  $ echo "invalid" > $HGRCPATH
  $ hg version
  hg: parse error at .*/\.hgrc:1: invalid (re)
  [255]
  $ echo "" > $HGRCPATH

issue1199: escaping

  $ hg init "foo%bar"
  $ hg clone "foo%bar" foobar
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ p=`pwd`
  $ cd foobar
  $ cat .hg/hgrc
  [paths]
  default = .*/foo%bar (re)
  $ hg paths
  default = .*/foo%bar (re)
  $ hg showconfig
  bundle\.mainreporoot=.*/foobar (re)
  paths\.default=.*/foo%bar (re)
  $ cd ..

issue1829: wrong indentation

  $ echo '[foo]' > $HGRCPATH
  $ echo '  x = y' >> $HGRCPATH
  $ hg version
  hg: parse error at .*/\.hgrc:2:   x = y (re)
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
  hg: parse error at .*/\.hgrc:1: cannot include /path/to/nowhere/no-such-file \(No such file or directory\) (re)
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
  read config from: .*/\.hgrc (re)
  .*/\.hgrc:13: alias\.log=log -g (re)
  .*/\.hgrc:11: defaults\.identify=-n (re)
  .*/\.hgrc:2: ui\.debug=true (re)
  .*/\.hgrc:3: ui\.fallbackencoding=ASCII (re)
  .*/\.hgrc:4: ui\.quiet=true (re)
  .*/\.hgrc:5: ui\.slash=true (re)
  .*/\.hgrc:6: ui\.traceback=true (re)
  .*/\.hgrc:7: ui\.verbose=true (re)
  .*/\.hgrc:8: ui\.style=~/.hgstyle (re)
  .*/\.hgrc:9: ui\.logtemplate=\{node\} (re)

plain hgrc

  $ HGPLAIN=; export HGPLAIN
  $ hg showconfig --config ui.traceback=True --debug
  read config from: .*/\.hgrc (re)
  none: ui.traceback=True
  none: ui.verbose=False
  none: ui.debug=True
  none: ui.quiet=False
