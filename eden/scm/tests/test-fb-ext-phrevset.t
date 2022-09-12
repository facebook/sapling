#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > phrevset=

  > [paths]
  > default=dummy://dummy
  > EOF
  $ hg init repo
  $ cd repo
  $ echo 1 > 1
  $ hg add 1
  $ hg commit -m 'Differential Revision: http.ololo.com/D1234'
  $ hg up -q 0
  $ hg up D1234
  phrevset.callsign is not set - doing a linear search
  This will be slow if the diff was not committed recently
  abort: phrevset.graphqlonly is set and Phabricator cannot resolve D1234
  [255]

  $ drawdag << 'EOS'
  > A  > EOS
  $ setconfig phrevset.mock-D1234=$A phrevset.callsign=CALLSIGN
  $ hg log -r D1234 -T '{desc}\n'
  A

# Callsign is invalid

  $ hg log -r D1234 --config phrevset.callsign=C -T '{desc}\n'
  abort: Diff callsign 'CALLSIGN' is different from repo callsigns '['C']'
  [255]

# Now we have two callsigns, and one of them is correct. Make sure it works

  $ hg log -r D1234 --config phrevset.callsign=C,CALLSIGN -T '{desc}\n'
  A

# Phabricator provides an unknown commit hash.

  $ setconfig phrevset.mock-D1234=6008bb23d775556ff6c3528541ca5a2177b4bb92
  $ hg log -r D1234 -T '{desc}\n'
  abort: unknown revision 'D1234'!
  [255]

# 'pull -r Dxxx' will be rewritten to 'pull -r HASH'

  $ hg pull -r D1234 --config paths.default=test:fake_server
  pulling from test:fake_server
  rewriting pull rev 'D1234' into '6008bb23d775556ff6c3528541ca5a2177b4bb92'
  abort: unknown revision '6008bb23d775556ff6c3528541ca5a2177b4bb92'!
  [255]
