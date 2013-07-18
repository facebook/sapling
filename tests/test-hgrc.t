hide outer repo
  $ hg init

Use hgrc within $TESTTMP

  $ HGRCPATH=`pwd`/hgrc
  $ export HGRCPATH

Use an alternate var for scribbling on hgrc to keep check-code from
complaining about the important settings we may be overwriting:

  $ HGRC=`pwd`/hgrc
  $ export HGRC

Basic syntax error

  $ echo "invalid" > $HGRC
  $ hg version
  hg: parse error at $TESTTMP/hgrc:1: invalid
  [255]
  $ echo "" > $HGRC

Issue1199: Can't use '%' in hgrc (eg url encoded username)

  $ hg init "foo%bar"
  $ hg clone "foo%bar" foobar
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd foobar
  $ cat .hg/hgrc
  [paths]
  default = $TESTTMP/foo%bar (glob)
  $ hg paths
  default = $TESTTMP/foo%bar (glob)
  $ hg showconfig
  bundle.mainreporoot=$TESTTMP/foobar (glob)
  paths.default=$TESTTMP/foo%bar (glob)
  $ cd ..

issue1829: wrong indentation

  $ echo '[foo]' > $HGRC
  $ echo '  x = y' >> $HGRC
  $ hg version
  hg: parse error at $TESTTMP/hgrc:2:   x = y
  [255]

  $ python -c "print '[foo]\nbar = a\n b\n c \n  de\n fg \nbaz = bif cb \n'" \
  > > $HGRC
  $ hg showconfig foo
  foo.bar=a\nb\nc\nde\nfg
  foo.baz=bif cb

  $ FAKEPATH=/path/to/nowhere
  $ export FAKEPATH
  $ echo '%include $FAKEPATH/no-such-file' > $HGRC
  $ hg version
  Mercurial Distributed SCM (version *) (glob)
  (see http://mercurial.selenic.com for more information)
  
  Copyright (C) 2005-2013 Matt Mackall and others
  This is free software; see the source for copying conditions. There is NO
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
  $ unset FAKEPATH

make sure global options given on the cmdline take precedence

  $ hg showconfig --config ui.verbose=True --quiet
  bundle.mainreporoot=$TESTTMP
  ui.verbose=False
  ui.debug=False
  ui.quiet=True

  $ touch foobar/untracked
  $ cat >> foobar/.hg/hgrc <<EOF
  > [ui]
  > verbose=True
  > EOF
  $ hg -R foobar st -q

username expansion

  $ olduser=$HGUSER
  $ unset HGUSER

  $ FAKEUSER='John Doe'
  $ export FAKEUSER
  $ echo '[ui]' > $HGRC
  $ echo 'username = $FAKEUSER' >> $HGRC

  $ hg init usertest
  $ cd usertest
  $ touch bar
  $ hg commit --addremove --quiet -m "added bar"
  $ hg log --template "{author}\n"
  John Doe
  $ cd ..

  $ hg showconfig
  bundle.mainreporoot=$TESTTMP
  ui.username=$FAKEUSER

  $ unset FAKEUSER
  $ HGUSER=$olduser
  $ export HGUSER

showconfig with multiple arguments

  $ echo "[alias]" > $HGRC
  $ echo "log = log -g" >> $HGRC
  $ echo "[defaults]" >> $HGRC
  $ echo "identify = -n" >> $HGRC
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

  $ echo "[ui]" > $HGRC
  $ echo "debug=true" >> $HGRC
  $ echo "fallbackencoding=ASCII" >> $HGRC
  $ echo "quiet=true" >> $HGRC
  $ echo "slash=true" >> $HGRC
  $ echo "traceback=true" >> $HGRC
  $ echo "verbose=true" >> $HGRC
  $ echo "style=~/.hgstyle" >> $HGRC
  $ echo "logtemplate={node}" >> $HGRC
  $ echo "[defaults]" >> $HGRC
  $ echo "identify=-n" >> $HGRC
  $ echo "[alias]" >> $HGRC
  $ echo "log=log -g" >> $HGRC

customized hgrc

  $ hg showconfig
  read config from: $TESTTMP/hgrc
  $TESTTMP/hgrc:13: alias.log=log -g
  none: bundle.mainreporoot=$TESTTMP
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
  none: bundle.mainreporoot=$TESTTMP
  none: ui.traceback=True
  none: ui.verbose=False
  none: ui.debug=True
  none: ui.quiet=False

plain mode with exceptions

  $ cat > plain.py <<EOF
  > def uisetup(ui):
  >     ui.write('plain: %r\n' % ui.plain())
  > EOF
  $ echo "[extensions]" >> $HGRC
  $ echo "plain=./plain.py" >> $HGRC
  $ HGPLAINEXCEPT=; export HGPLAINEXCEPT
  $ hg showconfig --config ui.traceback=True --debug
  plain: True
  read config from: $TESTTMP/hgrc
  none: bundle.mainreporoot=$TESTTMP
  $TESTTMP/hgrc:15: extensions.plain=./plain.py
  none: ui.traceback=True
  none: ui.verbose=False
  none: ui.debug=True
  none: ui.quiet=False
  $ unset HGPLAIN
  $ hg showconfig --config ui.traceback=True --debug
  plain: True
  read config from: $TESTTMP/hgrc
  none: bundle.mainreporoot=$TESTTMP
  $TESTTMP/hgrc:15: extensions.plain=./plain.py
  none: ui.traceback=True
  none: ui.verbose=False
  none: ui.debug=True
  none: ui.quiet=False
  $ HGPLAINEXCEPT=i18n; export HGPLAINEXCEPT
  $ hg showconfig --config ui.traceback=True --debug
  plain: True
  read config from: $TESTTMP/hgrc
  none: bundle.mainreporoot=$TESTTMP
  $TESTTMP/hgrc:15: extensions.plain=./plain.py
  none: ui.traceback=True
  none: ui.verbose=False
  none: ui.debug=True
  none: ui.quiet=False
