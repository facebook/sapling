#debugruntest-compatible
# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2006, 2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ cat > abortcommit.py << 'EOF'
  > from edenscm import error
  > def hook(**args):
  >     raise error.Abort("no commits allowed")
  > def reposetup(ui, repo):
  >     repo.ui.setconfig("hooks", "pretxncommit.nocommits", hook)
  > EOF
  $ abspath=`pwd`/abortcommit.py

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > mq =
  > abortcommit = $abspath
  > EOF

  $ hg init foo
  $ cd foo
  $ echo foo > foo
  $ hg add foo

# mq may keep a reference to the repository so __del__ will not be
# called and .hg/journal.dirstate will not be deleted:

  $ hg ci -m foo
  error: pretxncommit.nocommits hook failed: no commits allowed
  abort: no commits allowed
  [255]
  $ hg ci -m foo
  error: pretxncommit.nocommits hook failed: no commits allowed
  abort: no commits allowed
  [255]

  $ cd ..
