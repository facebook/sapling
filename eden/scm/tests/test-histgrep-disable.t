#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Simulate an environment that disables allowfullrepogrep:

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig 'histgrep.allowfullrepogrep=False'

# Test histgrep and check that it respects the specified file:

  $ hg init repo
  $ cd repo
  $ mkdir histgrepdir
  $ cd histgrepdir
  $ echo ababagalamaga > histgrepfile1
  $ echo ababagalamaga > histgrepfile2
  $ hg add histgrepfile1
  $ hg add histgrepfile2
  $ hg commit -m 'Added some files'
  $ hg histgrep ababagalamaga histgrepfile1
  histgrepdir/histgrepfile1:*:ababagalamaga (glob)
  $ hg histgrep ababagalamaga
  abort: can't run histgrep on the whole repo, please provide filenames
  (this is disabled to avoid very slow greps over the whole repo)
  [255]

# Now allow allowfullrepogrep:

  $ setconfig 'histgrep.allowfullrepogrep=True'
  $ hg histgrep ababagalamaga
  histgrepdir/histgrepfile1:*:ababagalamaga (glob)
  histgrepdir/histgrepfile2:*:ababagalamaga (glob)
  $ cd ..
