#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > arcconfig=$TESTDIR/../edenscm/ext/extlib/phabricator/arcconfig.py
  > EOF

# Sanity check expectations when there is no arcconfig

  $ hg init repo
  $ cd repo
  $ hg debugarcconfig
  abort: no .arcconfig found
  [255]

# Show that we can locate and reflect the contents of the .arcconfig from
# the repo dir

  $ echo '{"hello": "world"}' > .arcconfig
  $ hg debugarcconfig
  {"_arcconfig_path": "$TESTTMP/repo", "hello": "world"}

# We expect to see the combination of the user arcrc and the repo rc

  $ echo '{"user": true}' > $HOME/.arcrc
  $ hg debugarcconfig
  {"_arcconfig_path": "$TESTTMP/repo", "hello": "world", "user": true}
