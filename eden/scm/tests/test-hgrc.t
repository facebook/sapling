#chg-compatible

  $ configure modernclient

Use hgrc within $TESTTMP

  $ cp $HGRCPATH orig.hgrc
  $ HGRCPATH=`pwd`/hgrc
  $ export HGRCPATH
  $ cp orig.hgrc hgrc

Use an alternate var for scribbling on hgrc to keep check-code from
complaining about the important settings we may be overwriting:

  $ HGRC=`pwd`/hgrc
  $ export HGRC

Basic syntax error

  $ echo "invalid" > $HGRC
  $ hg version
  hg: parse errors: "$TESTTMP*hgrc": (glob)
  line 1: expect '[section]' or 'name = value'
  
  [255]
  $ cp orig.hgrc hgrc

Issue1199: Can't use '%' in hgrc (eg url encoded username)

  $ newclientrepo "foo%bar"
  $ newclientrepo foobar test:foo%bar_server
  $ cat .hg/hgrc
  
  [paths]
  default = test:foo%bar_server
  $ hg paths
  default = test:foo%EF%BF%BDr_server
  $ hg showconfig paths
  paths.default=test:foo%bar_server
  $ cd ..

issue1829: wrong indentation

  $ echo '[foo]' > $HGRC
  $ echo '  x = y' >> $HGRC
  $ hg version
  hg: parse errors: "$TESTTMP*hgrc": (glob)
  line 2: indented line is not part of a multi-line config
  
  [255]

  $ printf '[foo]\nbar = a\n b\n c \n  de\n fg \nbaz = bif cb \n' > $HGRC
  $ hg showconfig foo
  foo.bar=a\nb\nc\nde\nfg
  foo.baz=bif cb

  $ cp $TESTTMP/orig.hgrc $HGRC
  $ FAKEPATH=/path/to/nowhere
  $ export FAKEPATH
  $ echo '%include $FAKEPATH/no-such-file' >> $HGRC
  $ hg version
  Mercurial * (glob)
  $ unset FAKEPATH

make sure global options given on the cmdline take precedence

  $ hg showconfig --config ui.verbose=True --quiet ui
  ui.color=auto
  ui.debug=false
  ui.interactive=False
  ui.mergemarkers=detailed
  ui.paginate=true
  ui.promptecho=True
  ui.quiet=true
  ui.slash=True
  ui.ssh=* (glob)
  ui.timeout=600
  ui.verbose=false

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
  $ echo '[ui]' >> $HGRC
  $ echo 'username = $FAKEUSER' >> $HGRC

  $ newclientrepo usertest
  $ touch bar
  $ hg commit --addremove --quiet -m "added bar"
  $ hg log --template "{author}\n"
  John Doe
  $ cd ..

  $ hg showconfig | grep ui.username
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
  --config: ui.traceback=True
  --verbose: ui.verbose=False
  --debug: ui.debug=True
  --quiet: ui.quiet=False

with environment variables

  $ PAGER=p1 EDITOR=e1 VISUAL=e2 hg showconfig --debug
  $VISUAL: ui.editor=e2
  --verbose: ui.verbose=False
  --debug: ui.debug=True
  --quiet: ui.quiet=False

don't set editor to empty string

  $ VISUAL= hg showconfig --debug
  --verbose: ui.verbose=False
  --debug: ui.debug=True
  --quiet: ui.quiet=False

plain mode with exceptions

  $ cat > plain.py <<EOF
  > from edenscm import commands, extensions
  > def _config(orig, ui, repo, *values, **opts):
  >     ui.write('plain: %r\n' % ui.plain())
  >     return orig(ui, repo, *values, **opts)
  > def uisetup(ui):
  >     extensions.wrapcommand(commands.table, 'config', _config)
  > EOF
  $ echo "[extensions]" >> $HGRC
  $ echo "plain=./plain.py" >> $HGRC
  $ HGPLAINEXCEPT=; export HGPLAINEXCEPT
  $ hg showconfig --config ui.traceback=True --debug
  plain: True
  $TESTTMP/hgrc:15: extensions.plain=./plain.py
  --config: ui.traceback=True
  --verbose: ui.verbose=False
  --debug: ui.debug=True
  --quiet: ui.quiet=False
  $ unset HGPLAIN
  $ hg showconfig --config ui.traceback=True --debug
  plain: True
  $TESTTMP/hgrc:15: extensions.plain=./plain.py
  --config: ui.traceback=True
  --verbose: ui.verbose=False
  --debug: ui.debug=True
  --quiet: ui.quiet=False
  $ HGPLAINEXCEPT=i18n; export HGPLAINEXCEPT
  $ hg showconfig --config ui.traceback=True --debug
  plain: True
  $TESTTMP/hgrc:15: extensions.plain=./plain.py
  --config: ui.traceback=True
  --verbose: ui.verbose=False
  --debug: ui.debug=True
  --quiet: ui.quiet=False

source of paths is not mangled

  $ cat >> $HGRCPATH <<EOF
  > [paths]
  > foo = $TESTTMP/bar
  > EOF
  $ hg showconfig --debug paths
  plain: True
  $TESTTMP/hgrc:17: paths.foo=$TESTTMP/bar
