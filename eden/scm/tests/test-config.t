#debugruntest-compatible
#chg-compatible

hide outer repo
  $ setconfig workingcopy.ruststatus=False config.use-rust=True
  $ hg init

Invalid syntax: no value

  $ cat > .hg/hgrc << EOF
  > novaluekey
  > EOF
  $ hg showconfig
  hg: parse errors: "*hgrc": (glob)
  line 1: expect '[section]' or 'name = value'
  
  [255]

Invalid syntax: no key

  $ cat > .hg/hgrc << EOF
  > =nokeyvalue
  > EOF
  $ hg showconfig
  hg: parse errors: "*hgrc": (glob)
  line 1: empty config name
  
  [255]

Invalid syntax: content after section

  $ cat > .hg/hgrc << EOF
  > [section]#
  > EOF
  $ hg showconfig
  hg: parse errors: "*hgrc": (glob)
  line 1: extra content after section header
  
  [255]

Test hint about invalid syntax from leading white space

  $ cat > .hg/hgrc << EOF
  >  key=value
  > EOF
  $ hg showconfig
  hg: parse errors: "*hgrc": (glob)
  line 1: indented line is not part of a multi-line config
  
  [255]

  $ cat > .hg/hgrc << EOF
  >  [section]
  > key=value
  > EOF
  $ hg showconfig
  hg: parse errors: "*hgrc": (glob)
  line 1: indented line is not part of a multi-line config
  
  [255]

Reset hgrc

  $ echo > .hg/hgrc

Test case sensitive configuration

  $ cat <<EOF >> $HGRCPATH
  > [Section]
  > KeY = Case Sensitive
  > key = lower case
  > EOF

  $ hg showconfig Section
  Section.KeY=Case Sensitive
  Section.key=lower case

  $ hg showconfig Section -Tjson
  [
  {
    "name": "Section.KeY",
    "source": "*", (glob)
    "value": "Case Sensitive"
  },
  {
    "name": "Section.key",
    "source": "*", (glob)
    "value": "lower case"
  }
  ]
  $ hg showconfig Section.KeY -Tjson
  [
  {
    "name": "Section.KeY",
    "source": "*", (glob)
    "value": "Case Sensitive"
  }
  ]
  $ hg showconfig -Tjson | tail -7
  },
  {
    "name": "*", (glob)
    "source": "*", (glob)
    "value": "*" (glob)
  }
  ]

Test "%unset"

  $ cat >> $HGRCPATH <<EOF
  > [unsettest]
  > local-hgrcpath = should be unset (HGRCPATH)
  > %unset local-hgrcpath
  > 
  > global = should be unset (HGRCPATH)
  > 
  > both = should be unset (HGRCPATH)
  > 
  > set-after-unset = should be unset (HGRCPATH)
  > EOF

  $ cat >> .hg/hgrc <<EOF
  > [unsettest]
  > local-hgrc = should be unset (.hg/hgrc)
  > %unset local-hgrc
  > 
  > %unset global
  > 
  > both = should be unset (.hg/hgrc)
  > %unset both
  > 
  > set-after-unset = should be unset (.hg/hgrc)
  > %unset set-after-unset
  > set-after-unset = should be set (.hg/hgrc)
  > EOF

  $ hg showconfig unsettest
  unsettest.set-after-unset=should be set (.hg/hgrc)

Test exit code when no config matches

  $ hg config Section.idontexist
  [1]

sub-options in [paths] aren't expanded

  $ cat > .hg/hgrc << EOF
  > [paths]
  > foo = ~/foo
  > foo:suboption = ~/foo
  > EOF

  $ hg showconfig paths
  paths.foo=~/foo
  paths.foo:suboption=~/foo

edit failure

  $ HGEDITOR=false hg config --edit --quiet
  abort: edit failed: false exited with status 1
  [255]

  $ HGEDITOR=false hg config --user
  opening $TESTTMP/.hgrc for editing...
  abort: edit failed: false exited with status 1
  [255]

  $ hg config --user --local
  abort: please specify exactly one config location
  [255]

config affected by environment variables

  $ EDITOR=e1 hg config --debug | grep 'ui\.editor'
  $EDITOR: ui.editor=e1

  $ EDITOR=e2 hg config --debug --config ui.editor=e3 | grep 'ui\.editor'
  --config: ui.editor=e3

verify that aliases are evaluated as well

  $ hg init aliastest
  $ cd aliastest
  $ cat > .hg/hgrc << EOF
  > [ui]
  > user = repo user
  > EOF
  $ touch index
  $ unset HGUSER
  $ hg ci -Am test
  adding index
  $ hg log --template '{author}\n'
  repo user
  $ cd ..

alias has lower priority

  $ hg init aliaspriority
  $ cd aliaspriority
  $ cat > .hg/hgrc << EOF
  > [ui]
  > user = alias user
  > username = repo user
  > EOF
  $ touch index
  $ unset HGUSER
  $ hg ci -Am test
  adding index
  $ hg log --template '{author}\n'
  repo user
  $ cd ..

reponame is set from paths.default

  $ cat >> $HGRCPATH << EOF
  > [remotefilelog]
  > %unset reponame
  > EOF
  $ newrepo reponame-path-default-test
  $ enable remotenames
  $ hg paths --add default test:repo-myrepo1
  $ hg config remotefilelog.reponame
  repo-myrepo1
  $ cat .hg/reponame
  repo-myrepo1 (no-eol)

config editing without an editor

  $ newrepo

 invalid pattern
  $ hg config --edit missing.value
  abort: missing config value for 'missing.value'
  [255]

  $ hg config --edit missing=name
  abort: invalid argument: 'missing'
  (try section.name=value)
  [255]

 append configs
  $ hg config --local aa.bb.cc.字 "配
  > 置" ee.fff=gggg
  updated config in $TESTTMP/repo1/.hg/hgrc
  $ tail -6 .hg/hgrc | dos2unix
  [aa]
  bb.cc.字 = 配
    置
  
  [ee]
  fff = gggg

 update config in-place without appending
  $ hg config --local aa.bb.cc.字 new_值 "aa.bb.cc.字=新值
  > 测
  > 试
  > "
  updated config in $TESTTMP/repo1/.hg/hgrc
  $ tail -7 .hg/hgrc | dos2unix
  [aa]
  bb.cc.字 = 新值
    测
    试
  
  [ee]
  fff = gggg

  $ hg config aa.bb.cc.字
  新值\n测\n试

 with comments
  $ newrepo
  $ cat > .hg/hgrc << 'EOF'
  > [a]
  > # b = 1
  > b = 2
  >   second line
  > # b = 3
  > EOF

  $ hg config --local a.b 4
  updated config in $TESTTMP/repo2/.hg/hgrc
  $ cat .hg/hgrc
  [a]
  # b = 1
  b = 4
  # b = 3

  $ cd
  $ HGIDENTITY=sl newrepo
  $ sl config --local foo.bar baz
  updated config in $TESTTMP/repo3/.sl/config
  $ cat .sl/config | dos2unix
  # example repository config (see 'sl help config' for more info)
  [paths]
  # URL aliases to other repo sources
  # (see 'sl help config.paths' for more info)
  #
  # default = https://example.com/example-org/example-repo
  # my-fork = ssh://jdoe@example.com/jdoe/example-repo
  
  [ui]
  # name and email (local to this repository, optional), e.g.
  # username = Jane Doe <jdoe@example.com>
  
  [foo]
  bar = baz


 user config
  $ hg config --edit a.b=1 --quiet
  $ tail -2 ~/.hgrc | dos2unix
  [a]
  b = 1

  $ hg config --user a.b 2
  updated config in $TESTTMP/.hgrc
  $ tail -2 ~/.hgrc | dos2unix
  [a]
  b = 2

system config (make sure it tries the right file)
  $ PROGRAMDATA=C:\\ProgramData\\Facebook\\Mercurial\\ HGEDITOR=false hg config --system
  opening /etc/mercurial/system.rc for editing... (no-windows !)
  opening C:\ProgramData\Facebook\Mercurial\Facebook\Mercurial\system.rc for editing... (windows !)
  abort: edit failed: false exited with status 1
  [255]
