#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Set up extension

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > debugcommitmessage=
  > EOF

# Set up repo

  $ hg init repo
  $ cd repo

# Test extension

  $ hg debugcommitmessage
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: no files changed
  $ hg debugcommitmessage --config 'committemplate.changeset.commit.normal.normal=Test Specific Message\n'
  Test Specific Message
  $ hg debugcommitmessage --config 'committemplate.changeset.commit=Test Generic Message\n'
  Test Generic Message
  $ hg debugcommitmessage commit.amend.normal --config 'committemplate.changeset.commit=Test Generic Message\n'
  Test Generic Message
  $ hg debugcommitmessage randomform --config 'committemplate.changeset.commit=Test Generic Message\n'
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: no files changed
