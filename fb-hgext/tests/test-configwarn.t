  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > configwarn=$TESTDIR/../hgext3rd/configwarn.py
  > [configwarn]
  > systemconfigs=diff.git, phases.publish
  > [alias]
  > noop=log -r null -T '{files}'
  > EOF

Need to override rcutil.userrcpath to test properly without side-effects

  $ cat >> $TESTTMP/rcpath.py <<EOF
  > from __future__ import absolute_import
  > from mercurial import encoding, rcutil
  > def userrcpath():
  >     return [encoding.environ[b'USERHGRC']]
  > rcutil.userrcpath = userrcpath
  > EOF

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rcpath=$TESTTMP/rcpath.py
  > EOF

  $ USERHGRC="$TESTTMP/userhgrc"
  $ HGRCPATH="$HGRCPATH:$USERHGRC"
  $ export USERHGRC

Config set by system config files or command line flags are fine

  $ hg init
  $ hg noop
  $ hg noop --config diff.git=1

Config set by user config will generate warnings

  $ cat >> $USERHGRC <<EOF
  > [diff]
  > git=0
  > EOF

  $ hg noop
  warning: overriding config diff.git is unsupported (hint: remove line 2 from $TESTTMP/userhgrc to resolve this issue)

Config set by repo will generate warnings

  $ cat >> .hg/hgrc << EOF
  > [phases]
  > publish=1
  > EOF

  $ hg noop
  warning: overriding config diff.git is unsupported (hint: remove line 2 from $TESTTMP/userhgrc to resolve this issue)
  warning: overriding config phases.publish is unsupported (hint: remove line 2 from $TESTTMP/.hg/hgrc to resolve this issue)
