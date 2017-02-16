hide outer repo
  $ hg init

Invalid syntax: no value

  $ cat > .hg/hgrc << EOF
  > novaluekey
  > EOF
  $ hg showconfig
  hg: parse error at $TESTTMP/.hg/hgrc:1: novaluekey (glob)
  [255]

Invalid syntax: no key

  $ cat > .hg/hgrc << EOF
  > =nokeyvalue
  > EOF
  $ hg showconfig
  hg: parse error at $TESTTMP/.hg/hgrc:1: =nokeyvalue (glob)
  [255]

Test hint about invalid syntax from leading white space

  $ cat > .hg/hgrc << EOF
  >  key=value
  > EOF
  $ hg showconfig
  hg: parse error at $TESTTMP/.hg/hgrc:1:  key=value (glob)
  unexpected leading whitespace
  [255]

  $ cat > .hg/hgrc << EOF
  >  [section]
  > key=value
  > EOF
  $ hg showconfig
  hg: parse error at $TESTTMP/.hg/hgrc:1:  [section] (glob)
  unexpected leading whitespace
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
  read config from: * (glob)
  none: value
  $ hg config empty.source -Tjson
  [
   {
    "name": "empty.source",
    "source": "",
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
  paths.foo:suboption=~/foo
  paths.foo=$TESTTMP/foo
