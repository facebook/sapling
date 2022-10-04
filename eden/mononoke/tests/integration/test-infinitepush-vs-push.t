# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ SCUBA_LOGGING_PATH="$TESTTMP/scuba.json"
  $ INFINITEPUSH_ALLOW_WRITES=true setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend=
  > infinitepush=
  > commitcloud=
  > EOF

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg addremove && hg ci -q -ma
  adding a

create master bookmark
  $ hg bookmark master_bookmark -r tip
  $ cd $TESTTMP

setup repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ cat >> "$TESTTMP/repo-push/.hg/hgrc" <<EOF
  > [extensions]
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF

blobimport

  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke --scuba-dataset "file://$SCUBA_LOGGING_PATH"
  $ wait_for_mononoke


Do infinitepush (aka commit cloud) push
  $ cd repo-push
  $ hg up -q tip
  $ hg ci -m new --config ui.allowemptycommit=True
  $ hgmn pushbackup -r .
  backing up stack rooted at * (glob)
  commitcloud: backed up 1 commit

  $ killandwait $MONONOKE_PID

At least once infinitepush was performed
  $ jq '.normal | contains({log_tag: "Unbundle resolved", msg: "infinitepush"})' < "$SCUBA_LOGGING_PATH" | grep true
  true
