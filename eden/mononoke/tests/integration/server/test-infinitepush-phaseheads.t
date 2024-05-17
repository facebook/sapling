# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

  $ function graphlog() {
  >   hg log -G -T "{node|short} {phase} '{desc}' {bookmarks} {remotebookmarks}" "$@"
  > }

setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
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
  $ touch base
  $ hg commit -Aqm base
  $ echo 1 > file
  $ hg commit -Aqm public1

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-push and repo-pull
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate --config extensions.remotenames=
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate --config extensions.remotenames=

blobimport

  $ blobimport repo-hg/.hg repo

start mononoke

  $ start_and_wait_for_mononoke_server
push some draft commits
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 1 > newfile
  $ hg addremove -q
  $ hg commit -m draft1
  $ echo 2 >> newfile
  $ hg commit -m draft2
  $ hgmn push mononoke://$(mononoke_address)/repo -r . --bundle-store --allow-anon
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes

  $ graphlog
  @  fc8f2fba9ac9 draft 'draft2'
  │
  o  48337b947baa draft 'draft1'
  │
  o  f2f073d106b0 public 'public1'  default/master_bookmark
  │
  o  df4f53cec30a public 'base'
  

pull these draft commits
  $ cd "$TESTTMP/repo-pull"
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ hgmn pull -r fc8f2fba9ac9
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes

  $ graphlog
  o  fc8f2fba9ac9 draft 'draft2'
  │
  o  48337b947baa draft 'draft1'
  │
  o  f2f073d106b0 public 'public1'  default/master_bookmark
  │
  o  df4f53cec30a public 'base'
  

land the first draft commit
  $ cd "$TESTTMP/repo-push"
  $ hgmn push -r 48337b947baa --to master_bookmark
  pushing rev 48337b947baa to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

put a new draft commit on top
  $ echo 3 >> newfile
  $ hg commit -m draft3
  $ hgmn push mononoke://$(mononoke_address)/repo -r . --bundle-store --allow-anon
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes

add a new public branch
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo 1 > branchfile
  $ hg commit -Aqm branch1
  $ hgmn push -r . --to branch_bookmark --create
  pushing rev eaf82af99127 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark branch_bookmark
  searching for changes
  exporting bookmark branch_bookmark

add some draft commits to the branch
  $ echo 2 >> branchfile
  $ hg commit -Aqm branch2
  $ echo 3 >> branchfile
  $ hg commit -Aqm branch3
  $ hgmn push mononoke://$(mononoke_address)/repo -r . --bundle-store --allow-anon
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes

  $ graphlog
  @  3e86159717e8 draft 'branch3'
  │
  o  0bf099b792a8 draft 'branch2'
  │
  o  eaf82af99127 public 'branch1'  default/branch_bookmark
  │
  │ o  09b17e5ff090 draft 'draft3'
  │ │
  │ o  fc8f2fba9ac9 draft 'draft2'
  │ │
  │ o  48337b947baa public 'draft1'  default/master_bookmark
  │ │
  │ o  f2f073d106b0 public 'public1'
  ├─╯
  o  df4f53cec30a public 'base'
  

pull all of these commits
  $ cd "$TESTTMP/repo-pull"
  $ hgmn pull -r 09b17e5ff090 -r 3e86159717e8
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes

the server will have returned phaseheads information that makes 'draft1' and
'branch1' public, and everything else draft
  $ graphlog
  o  09b17e5ff090 draft 'draft3'
  │
  │ o  3e86159717e8 draft 'branch3'
  │ │
  │ o  0bf099b792a8 draft 'branch2'
  │ │
  │ o  eaf82af99127 public 'branch1'  default/branch_bookmark
  │ │
  o │  fc8f2fba9ac9 draft 'draft2'
  │ │
  o │  48337b947baa public 'draft1'  default/master_bookmark
  │ │
  o │  f2f073d106b0 public 'public1'
  ├─╯
  o  df4f53cec30a public 'base'
  

