# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Test that bookmark updates during discovery don't cause problems for pulls
running concurrently. See the comment in mononoke/server/src/repo.rs:bundle2caps
for more.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup master bookmarks

  $ hg bookmark master_bookmark -r 'tip'

  $ cd $TESTTMP
  $ blobimport repo/.hg repo

start mononoke

  $ start_and_wait_for_mononoke_server
setup two repos: one will be used to pull into, and one will be used to
update master_bookmark concurrently.

  $ hg clone -q mono:repo repo-pull

  $ hg clone -q mono:repo repo-push
  $ cd repo-push
  $ hg up master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "b file content" > b
  $ hg add b
  $ hg ci -mb
  $ hg push -r . --allow-anon
  pushing to mono:repo
  searching for changes
  $ echo "c file content" > c
  $ hg add c
  $ hg ci -mc

  $ cd $TESTTMP/repo-pull

configure an extension so that a push happens right after pulldiscovery

  $ cat > $TESTTMP/pulldiscovery_push.py << EOF
  > from edenscm import (
  >     exchange,
  >     extensions,
  > )
  > def wrappulldiscovery(orig, pullop):
  >     pullop.repo.ui.write("*** starting discovery\n")
  >     orig(pullop)
  >     pullop.repo.ui.write("*** running push\n")
  >     pullop.repo.ui.system(
  >         "bash -c 'source \"${TEST_FIXTURES}/library.sh\"; hg push -R $TESTTMP/repo-push --allow-anon'",
  >         onerr=lambda str: Exception(str),
  >     )
  >     pullop.repo.ui.write("*** push complete\n")
  > def extsetup(ui):
  >     extensions.wrapfunction(exchange, '_pulldiscovery', wrappulldiscovery)
  > EOF

  $ hg pull --config extensions.pulldiscovery_push=$TESTTMP/pulldiscovery_push.py
  pulling from mono:repo
  *** starting discovery
  searching for changes
  *** running push
  pushing to mono:repo
  searching for changes
  *** push complete
  no changes found
  adding changesets
  adding manifests
  adding file changes

  $ hg bookmarks --all
  no bookmarks set
     default/master_bookmark   0e7ec5675652

pull again to ensure the new version makes it into repo-pull

  $ hg pull
  pulling from mono:repo
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes
  $ hg bookmarks --all
  no bookmarks set
     default/master_bookmark   0e7ec5675652
