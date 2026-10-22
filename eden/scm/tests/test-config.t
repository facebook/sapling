
#require no-eden

hide outer repo
  $ sl init

Invalid syntax: no value

  $ cat > .sl/config << EOF
  > novaluekey
  > EOF
  $ sl config
  sl: parse errors: "*config": (glob)
  line 1: expect '[section]' or 'name = value'
  
  [255]

Invalid syntax: no key

  $ cat > .sl/config << EOF
  > =nokeyvalue
  > EOF
  $ sl config
  sl: parse errors: "*config": (glob)
  line 1: empty config name
  
  [255]

Invalid syntax: content after section

  $ cat > .sl/config << EOF
  > [section]#
  > EOF
  $ sl config
  sl: parse errors: "*config": (glob)
  line 1: extra content after section header
  
  [255]

Test hint about invalid syntax from leading white space

  $ cat > .sl/config << EOF
  >  key=value
  > EOF
  $ sl config
  sl: parse errors: "*config": (glob)
  line 1: indented line is not part of a multi-line config
  
  [255]

  $ cat > .sl/config << EOF
  >  [section]
  > key=value
  > EOF
  $ sl config
  sl: parse errors: "*config": (glob)
  line 1: indented line is not part of a multi-line config
  
  [255]

Reset hgrc

  $ echo > .sl/config

Test case sensitive configuration

  $ cat <<EOF >> $HGRCPATH
  > [Section]
  > KeY = Case Sensitive
  > key = lower case
  > EOF

  $ sl config Section
  Section.KeY=Case Sensitive
  Section.key=lower case

  $ sl config Section -Tjson
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
  $ sl config Section.KeY -Tjson
  [
  {
    "name": "Section.KeY",
    "source": "*", (glob)
    "value": "Case Sensitive"
  }
  ]
  $ sl config -Tjson | tail -7
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

  $ cat >> .sl/config <<EOF
  > [unsettest]
  > local-hgrc = should be unset (.sl/config)
  > %unset local-hgrc
  > 
  > %unset global
  > 
  > both = should be unset (.sl/config)
  > %unset both
  > 
  > set-after-unset = should be unset (.sl/config)
  > %unset set-after-unset
  > set-after-unset = should be set (.sl/config)
  > EOF

  $ sl config unsettest
  unsettest.set-after-unset=should be set (.sl/config)

  $ sl config unsettest.both
  [1]
  $ sl config unsettest.both --debug
  *: <%unset> (glob)

  $ sl config unsettest.both -Tjson
  [
  ]
  $ sl config unsettest.both -Tjson --debug
  [
  {
    "name": "unsettest.both",
    "source": "*", (glob)
    "value": null
  }
  ]

Read multiple values with -Tjson

  $ sl config --config=a.1=1 --config=a.2=2 --config=a.3=3 a.1 a.3 -Tjson
  [
  {
    "name": "a.1",
    "source": "--config",
    "value": "1"
  },
  {
    "name": "a.3",
    "source": "--config",
    "value": "3"
  }
  ]

  $ sl config --config=a.1=1 --config=a.2=2 --config=a.3=3 --config=b.1=4 --config=b.2=5 a.3 b a.1 -Tjson
  [
  {
    "name": "a.3",
    "source": "--config",
    "value": "3"
  },
  {
    "name": "b.1",
    "source": "--config",
    "value": "4"
  },
  {
    "name": "b.2",
    "source": "--config",
    "value": "5"
  },
  {
    "name": "a.1",
    "source": "--config",
    "value": "1"
  }
  ]

Config order is preserved:

  $ cat <<EOF >> $HGRCPATH
  > [ordertest]
  > a = 1
  > c = 3
  > b = 2
  > EOF

  $ sl config ordertest
  ordertest.a=1
  ordertest.c=3
  ordertest.b=2

Test exit code when no config matches

  $ sl config Section.idontexist
  [1]

sub-options in [paths] aren't expanded

  $ cat > .sl/config << EOF
  > [paths]
  > foo = ~/foo
  > foo:suboption = ~/foo
  > EOF

  $ sl config paths
  paths.foo=~/foo
  paths.foo:suboption=~/foo

config affected by environment variables

  $ EDITOR=e1 sl config --debug | grep 'ui\.editor'
  $EDITOR: ui.editor=e1

  $ EDITOR=e2 sl config --debug --config ui.editor=e3 | grep 'ui\.editor'
  --config: ui.editor=e3

verify that aliases are evaluated as well

  $ sl init aliastest
  $ cd aliastest
  $ cat > .sl/config << EOF
  > [ui]
  > user = repo user
  > EOF
  $ touch index
  $ unset HGUSER
  $ sl ci -Am test
  adding index
  $ sl log --template '{author}\n'
  repo user
  $ cd ..

alias has lower priority

  $ sl init aliaspriority
  $ cd aliaspriority
  $ cat > .sl/config << EOF
  > [ui]
  > user = alias user
  > username = repo user
  > EOF
  $ touch index
  $ unset HGUSER
  $ sl ci -Am test
  adding index
  $ sl log --template '{author}\n'
  repo user
  $ cd ..

reponame is set from paths.default

  $ cat >> $HGRCPATH << EOF
  > [remotefilelog]
  > %unset reponame
  > EOF
  $ newrepo reponame-path-default-test
  $ sl paths --add default test:repo-myrepo1
  $ sl config remotefilelog.reponame
  repo-myrepo1
  $ cat .sl/reponame
  repo-myrepo1 (no-eol)

Show builtin configs with --verbose (filtersuspectsymlink is merely a sample item from builtin:core):
  $ sl config | grep filtersuspectsymlink || true
  $ sl config --verbose | grep filtersuspectsymlink
  unsafe.filtersuspectsymlink=true

Can see all sources w/ --debug and --verbose:
  $ newrepo sources
  $ cat > .sl/config << EOF
  > %include $TESTTMP/sources.rc
  > [foo]
  > bar = 1
  > [foo]
  > bar = 2
  > EOF

  $ cat > $TESTTMP/sources.rc << EOF
  > [foo]
  > bar = 3
  > EOF

  $ sl config foo.bar --debug --verbose
  $TESTTMP/sources/.sl/config:*: 2 (glob)
    $TESTTMP/sources/.sl/config:*: 1 (glob)
    $TESTTMP/sources.rc:*: 3 (glob)

  $ sl config foo --debug --verbose
  $TESTTMP/sources/.sl/config:*: foo.bar=2 (glob)
    $TESTTMP/sources/.sl/config:*: foo.bar=1 (glob)
    $TESTTMP/sources.rc:*: foo.bar=3 (glob)
