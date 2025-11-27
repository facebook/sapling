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
  > commitcloud=
  > EOF
  $ setconfig pull.use-commit-graph=true
  $ setconfig remotenames.selectivepulldefault=master_bookmark,branch_bookmark

setup repo

  $ hginit_treemanifest repo
  $ cd repo

Create commits using testtool drawdag
  $ testtool_drawdag -R repo --no-default-files <<'EOF'
  > A-B
  > # modify: A "base" "base\n"
  > # modify: B "file" "1\n"
  > # bookmark: B master_bookmark
  > EOF
  A=78dc0344b2581a22b30196955ce8d96dc5aa3ebf0f25dec2bb995dde56d628c7
  B=691345013abe1db6b55a7cbdbf29fa4a06c7f0cc9ad2ef740939e9b6e07846ea

Import and start mononoke
  $ cd "$TESTTMP"
  $ mononoke
  $ wait_for_mononoke

setup repo-push and repo-pull
  $ hg clone -q mono:repo repo-push --noupdate
  $ hg clone -q mono:repo repo-pull --noupdate
push some draft commits
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
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
  $ hg cloud upload -qr .

  $ DRAFT2=$(hg log -r . -T '{node}')

  $ graphlog
  @  4d5af358d6be draft 'draft2'
  │
  o  4617682a08e7 draft 'draft1'
  │
  o  27e51f947972 public 'B'  remote/master_bookmark
  │
  o  4b8f980e0603 public 'A'
  


pull these draft commits
  $ cd "$TESTTMP/repo-pull"
  $ cat >> .hg/hgrc <<EOF
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ hg pull -qr $DRAFT2

  $ graphlog
  o  4d5af358d6be draft 'draft2'
  │
  o  4617682a08e7 draft 'draft1'
  │
  o  27e51f947972 public 'B'  remote/master_bookmark
  │
  o  4b8f980e0603 public 'A'
  


  $ DRAFT1=$(hg log -r 'desc(draft1)' -T '{node}')

land the first draft commit
  $ cd "$TESTTMP/repo-push"
  $ hg push -r $DRAFT1 --to master_bookmark
  pushing rev 4617682a08e7 to destination mono:repo bookmark master_bookmark
  searching for changes
  no changes found
  updating bookmark master_bookmark

put a new draft commit on top
  $ echo 3 >> newfile
  $ hg commit -m draft3
  $ hg cloud upload -qr .
  $ DRAFT3=$(hg log -r . -T '{node}')

add a new public branch
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo 1 > branchfile
  $ hg commit -Aqm branch1
  $ BRANCH1=$(hg log -r . -T '{node}')
  $ hg push -r . --to branch_bookmark --create
  pushing rev 0fa1940e61f9 to destination mono:repo bookmark branch_bookmark
  searching for changes
  exporting bookmark branch_bookmark

add some draft commits to the branch
  $ echo 2 >> branchfile
  $ hg commit -Aqm branch2
  $ echo 3 >> branchfile
  $ hg commit -Aqm branch3
  $ hg cloud upload -qr .
  $ BRANCH3=$(hg log -r . -T '{node}')

  $ graphlog
  @  700ba7b49456 draft 'branch3'
  │
  o  a367c6aa3621 draft 'branch2'
  │
  o  0fa1940e61f9 public 'branch1'  remote/branch_bookmark
  │
  │ o  20f63ac77b20 draft 'draft3'
  │ │
  │ o  4d5af358d6be draft 'draft2'
  │ │
  │ o  4617682a08e7 public 'draft1'  remote/master_bookmark
  │ │
  │ o  27e51f947972 public 'B'
  ├─╯
  o  4b8f980e0603 public 'A'
  


pull all of these commits
  $ cd "$TESTTMP/repo-pull"
  $ hg pull -r $DRAFT3 -r $BRANCH3
  pulling from mono:repo
  searching for changes

the server will have returned phaseheads information that makes 'draft1' and
'branch1' public, and everything else draft
  $ graphlog
  o  700ba7b49456 draft 'branch3'
  │
  o  a367c6aa3621 draft 'branch2'
  │
  o  0fa1940e61f9 public 'branch1'  remote/branch_bookmark
  │
  │ o  20f63ac77b20 draft 'draft3'
  │ │
  │ o  4d5af358d6be draft 'draft2'
  │ │
  │ o  4617682a08e7 public 'draft1'  remote/master_bookmark
  │ │
  │ o  27e51f947972 public 'B'
  ├─╯
  o  4b8f980e0603 public 'A'
  

