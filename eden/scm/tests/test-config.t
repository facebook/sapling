#chg-compatible

hide outer repo
  $ hg init

Invalid syntax: no value

  $ cat > .hg/hgrc << EOF
  > novaluekey
  > EOF
  $ hg showconfig
  hg: parse error: "$TESTTMP/.hg/hgrc":
   --> 1:11
    |
  1 | novaluekey\xe2\x90\x8a (esc)
    |           ^---
    |
    = expected equal_sign
  [255]

Invalid syntax: no key

  $ cat > .hg/hgrc << EOF
  > =nokeyvalue
  > EOF
  $ hg showconfig
  hg: parse error: "$TESTTMP/.hg/hgrc":
   --> 1:1
    |
  1 | =nokeyvalue\xe2\x90\x8a (esc)
    | ^---
    |
    = expected EOI, new_line, config_name, left_bracket, comment_line, or directive
  [255]

Test hint about invalid syntax from leading white space

  $ cat > .hg/hgrc << EOF
  >  key=value
  > EOF
  $ hg showconfig
  hg: parse error: "$TESTTMP/.hg/hgrc":
   --> 1:2
    |
  1 |  key=value\xe2\x90\x8a (esc)
    |  ^---
    |
    = expected EOI or new_line
  [255]

  $ cat > .hg/hgrc << EOF
  >  [section]
  > key=value
  > EOF
  $ hg showconfig
  hg: parse error: "$TESTTMP/.hg/hgrc":
   --> 1:2
    |
  1 |  [section]\xe2\x90\x8a (esc)
    |  ^---
    |
    = expected EOI or new_line
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
    "source": "*.hgrc:*", (glob)
    "value": "Case Sensitive"
   },
   {
    "name": "Section.key",
    "source": "*.hgrc:*", (glob)
    "value": "lower case"
   }
  ]
  $ hg showconfig Section.KeY -Tjson
  [
   {
    "name": "Section.KeY",
    "source": "*.hgrc:*", (glob)
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

Test empty config source:

  $ cat <<EOF > emptysource.py
  > def reposetup(ui, repo):
  >     ui.setconfig('empty', 'source', 'value')
  > EOF
  $ cp .hg/hgrc .hg/hgrc.orig
  $ cat <<EOF >> .hg/hgrc
  > [extensions]
  > emptysource = `pwd`/emptysource.py
  > EOF

  $ hg config --debug empty.source
  ui.setconfig: value
  $ hg config empty.source -Tjson
  [
   {
    "name": "empty.source",
    "source": "ui.setconfig",
    "value": "value"
   }
  ]

  $ cp .hg/hgrc.orig .hg/hgrc

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
  paths.foo=$TESTTMP/foo
  paths.foo:suboption=~/foo

edit failure

  $ HGEDITOR=false hg config --edit
  abort: edit failed: false exited with status 1
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
