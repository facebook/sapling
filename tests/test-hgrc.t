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
  # example repository config (see "hg help config" for more info)
  [paths]
  default = $TESTTMP/foo%bar (glob)
  
  # path aliases to other clones of this repo in URLs or filesystem paths
  # (see "hg help config.paths" for more info)
  #
  # default-push = ssh://jdoe@example.net/hg/jdoes-fork
  # my-fork      = ssh://jdoe@example.net/hg/jdoes-fork
  # my-clone     = /home/jdoe/jdoes-clone
  
  [ui]
  # name and email (local to this repository, optional), e.g.
  # username = Jane Doe <jdoe@example.com>
  $ hg paths
  default = $TESTTMP/foo%bar (glob)
  $ hg showconfig
  bundle.mainreporoot=$TESTTMP/foobar (glob)
  extensions.chgserver= (?)
  paths.default=$TESTTMP/foo%bar (glob)
  $ cd ..

issue1829: wrong indentation

  $ echo '[foo]' > $HGRC
  $ echo '  x = y' >> $HGRC
  $ hg version
  hg: parse error at $TESTTMP/hgrc:2:   x = y
  unexpected leading whitespace
  [255]

  $ $PYTHON -c "print '[foo]\nbar = a\n b\n c \n  de\n fg \nbaz = bif cb \n'" \
  > > $HGRC
  $ hg showconfig foo
  foo.bar=a\nb\nc\nde\nfg
  foo.baz=bif cb

  $ FAKEPATH=/path/to/nowhere
  $ export FAKEPATH
  $ echo '%include $FAKEPATH/no-such-file' > $HGRC
  $ hg version
  Mercurial Distributed SCM (version *) (glob)
  (see https://mercurial-scm.org for more information)
  
  Copyright (C) 2005-2016 Matt Mackall and others
  This is free software; see the source for copying conditions. There is NO
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
  $ unset FAKEPATH

make sure global options given on the cmdline take precedence

  $ hg showconfig --config ui.verbose=True --quiet
  bundle.mainreporoot=$TESTTMP
  extensions.chgserver= (?)
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
  extensions.chgserver= (?)
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
  repo: bundle.mainreporoot=$TESTTMP
  $TESTTMP/hgrc:11: defaults.identify=-n
  --config: extensions.chgserver= (?)
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
  repo: bundle.mainreporoot=$TESTTMP
  --config: extensions.chgserver= (?)
  --config: ui.traceback=True
  --verbose: ui.verbose=False
  --debug: ui.debug=True
  --quiet: ui.quiet=False

plain mode with exceptions

  $ cat > plain.py <<EOF
  > from mercurial import commands, extensions
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
  read config from: $TESTTMP/hgrc
  repo: bundle.mainreporoot=$TESTTMP
  $TESTTMP/hgrc:15: extensions.plain=./plain.py
  --config: extensions.chgserver= (?)
  --config: ui.traceback=True
  --verbose: ui.verbose=False
  --debug: ui.debug=True
  --quiet: ui.quiet=False
  $ unset HGPLAIN
  $ hg showconfig --config ui.traceback=True --debug
  plain: True
  read config from: $TESTTMP/hgrc
  repo: bundle.mainreporoot=$TESTTMP
  $TESTTMP/hgrc:15: extensions.plain=./plain.py
  --config: extensions.chgserver= (?)
  --config: ui.traceback=True
  --verbose: ui.verbose=False
  --debug: ui.debug=True
  --quiet: ui.quiet=False
  $ HGPLAINEXCEPT=i18n; export HGPLAINEXCEPT
  $ hg showconfig --config ui.traceback=True --debug
  plain: True
  read config from: $TESTTMP/hgrc
  repo: bundle.mainreporoot=$TESTTMP
  $TESTTMP/hgrc:15: extensions.plain=./plain.py
  --config: extensions.chgserver= (?)
  --config: ui.traceback=True
  --verbose: ui.verbose=False
  --debug: ui.debug=True
  --quiet: ui.quiet=False

source of paths is not mangled

  $ cat >> $HGRCPATH <<EOF
  > [paths]
  > foo = bar
  > EOF
  $ hg showconfig --debug paths
  plain: True
  read config from: $TESTTMP/hgrc
  $TESTTMP/hgrc:17: paths.foo=$TESTTMP/bar (glob)
