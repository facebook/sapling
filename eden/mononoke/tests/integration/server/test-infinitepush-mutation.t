# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests
  $ enable amend commitcloud
  $ setconfig ui.ssh="\"$DUMMYSSH\""
  $ setconfig mutation.date="0 0" mutation.enabled=true mutation.record=true visibility.enabled=true
  $ setconfig experimental.evolution=obsolete
  $ setconfig remotenames.selectivepull=true

setup repo

  $ hginit_treemanifest repo
  $ cd repo

Create commits using testtool drawdag
  $ testtool_drawdag -R repo --no-default-files <<'EOF'
  > A
  > # modify: A "base" "base\n"
  > # bookmark: A master_bookmark
  > EOF
  A=78dc0344b2581a22b30196955ce8d96dc5aa3ebf0f25dec2bb995dde56d628c7

Import and start mononoke
  $ cd "$TESTTMP"
  $ mononoke
  $ wait_for_mononoke

setup repo-push and repo-pull
  $ hg clone -q mono:repo repo-push --noupdate
  $ hg clone -q mono:repo repo-pull --noupdate
  $ cd $TESTTMP/repo-push
  $ setconfig infinitepush.server=false infinitepush.branchpattern="re:scratch/.+"

  $ cd $TESTTMP/repo-pull
  $ setconfig infinitepush.server=false infinitepush.branchpattern="re:scratch/.+"

Do initial infinitepush of a small stack
  $ cd $TESTTMP/repo-push
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 1 > A
  $ hg commit -Aqm A1
  $ echo 1 > B
  $ hg commit -Aqm B1
  $ tglogp
  @  7942b2903aa9 draft 'B1'
  │
  o  1f8b3551d39e draft 'A1'
  │
  o  4b8f980e0603 public 'A'
  
  $ hg cloud upload -qr .

Amend the bottom commit
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [1f8b35] A1
  $ echo 2 > A
  $ hg amend -qm A2 --rebase
  $ hg next
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [aa67c1] B1
  $ tglogp
  @  aa67c19e7b87 draft 'B1'
  │
  o  be397765dd77 draft 'A2'
  │
  o  4b8f980e0603 public 'A'
  
  $ hg cloud upload -qr .
  $ hg debugmutation -r "draft()"
   *  be397765dd778e1225486300e310a67ddff91237 amend by test at 1970-01-01T00:00:00 from:
      1f8b3551d39edf7570413bbec6bacb0583363b54
  
   *  aa67c19e7b879157d46041c7099fde3bafa0d6ce rebase by test at 1970-01-01T00:00:00 from:
      7942b2903aa93cff8e0533ff7fb9b327c4e5f621
  


  $ COMMIT1=$(hg log -r . -T '{node}')

Pull the amended stack to the other repo
  $ cd $TESTTMP/repo-pull
  $ hg pull -r $COMMIT1
  pulling from mono:repo
  searching for changes
  $ tglogp
  o  aa67c19e7b87 draft 'B1'
  │
  o  be397765dd77 draft 'A2'
  │
  o  4b8f980e0603 public 'A'
  

Check mutation metadata.
  $ hg debugmutation -r "draft()"
   *  be397765dd778e1225486300e310a67ddff91237 amend by test at 1970-01-01T00:00:00 from:
      1f8b3551d39edf7570413bbec6bacb0583363b54
  
   *  aa67c19e7b879157d46041c7099fde3bafa0d6ce rebase by test at 1970-01-01T00:00:00 from:
      7942b2903aa93cff8e0533ff7fb9b327c4e5f621
  
Amend the stack again.
  $ cd $TESTTMP/repo-push
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [be3977] A2
  $ echo 3 > A
  $ hg amend -qm A3 --rebase
  $ hg next
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [0ff72d] B1
  $ hg cloud upload -qr .
  $ COMMIT2=$(hg log -r . -T '{node}')

Pull the amended stack to the other repo.
  $ cd $TESTTMP/repo-pull
  $ hg pull -r $COMMIT2
  pulling from mono:repo
  searching for changes
  $ tglogm
  o  0ff72db828fc 'B1'
  │
  o  4bc9a1383b3f 'A3'
  │
  │ x  aa67c19e7b87 'B1'  (Rewritten using rebase into 0ff72db828fc)
  │ │
  │ x  be397765dd77 'A2'  (Rewritten using amend into 4bc9a1383b3f)
  ├─╯
  o  4b8f980e0603 'A'
  

Do some more complicated mutations
  $ cd $TESTTMP/repo-push
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [4bc9a1] A3
  $ echo 1 > C
  $ hg commit -Aqm C1
  $ echo 2 > C
  $ hg amend -qm C2
  $ echo 3 > C
  $ hg amend -qm C3
  $ hg fold --from ".^"
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  rebasing 0ff72db828fc "B1"
  $ hg next
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [0ae3fa] B1
  $ tglogm
  @  0ae3fa2ed166 'B1'
  │
  o  763712322f9d 'A3'
  │
  o  4b8f980e0603 'A'
  

  $ hg cloud upload -qr .
  $ COMMIT3=$(hg log -r . -T '{node}')

Pull the modified stack to the other repo.
  $ cd $TESTTMP/repo-pull
  $ hg pull -r $COMMIT3
  pulling from mono:repo
  searching for changes
  $ tglogm
  o  0ae3fa2ed166 'B1'
  │
  o  763712322f9d 'A3'
  │
  │ x  0ff72db828fc 'B1'  (Rewritten using rebase into 0ae3fa2ed166)
  │ │
  │ x  4bc9a1383b3f 'A3'  (Rewritten using fold into 763712322f9d)
  ├─╯
  │ x  aa67c19e7b87 'B1'  (Rewritten using rebase into 0ff72db828fc)
  │ │
  │ x  be397765dd77 'A2'  (Rewritten using amend into 4bc9a1383b3f)
  ├─╯
  o  4b8f980e0603 'A'
  
