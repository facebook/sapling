#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Set up

  $ cat >> $HGRCPATH << 'EOF'
  > [experimental]
  > evolution=all
  > [extensions]
  > amend=
  > tweakdefaults=
  > EOF

# Test hg bookmark works with hidden commits

  $ hg init repo1
  $ cd repo1
  $ touch a
  $ hg commit -A a -m a
  $ echo 1 >> a
  $ hg commit a -m a1
  $ hg hide da7a5140a611 -q
  $ hg bookmark b -r da7a5140a611 -q

# Same test but with remotenames enabled

  $ hg bookmark b2 -r da7a5140a611 -q --config 'extensions.remotenames='
