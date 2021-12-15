# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ cd $TESTTMP
  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "mutation_advertise_for_infinitepush": true,
  >     "mutation_accept_for_infinitepush": true,
  >     "mutation_generate_for_draft": true
  >   }
  > }
  > EOF

setup common configuration for these tests
  $ enable amend infinitepush commitcloud remotenames
  $ setconfig ui.ssh="\"$DUMMYSSH\""
  $ setconfig mutation.date="0 0" mutation.enabled=true mutation.record=true visibility.enabled=true
  $ setconfig experimental.evolution=obsolete

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch base
  $ hg commit -Aqm base
  $ tglogp
  @  df4f53cec30a draft 'base'
  

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-push and repo-pull
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate

blobimport

  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke

  $ cd $TESTTMP/repo-push
  $ enable remotenames
  $ setconfig infinitepush.server=false infinitepush.branchpattern="re:scratch/.+"

  $ cd $TESTTMP/repo-pull
  $ enable remotenames
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
  @  f99c737e05b5 draft 'B1'
  │
  o  9b5a540873ab draft 'A1'
  │
  o  df4f53cec30a public 'base'
  
  $ hgmn push ssh://user@dummy/repo -r . --bundle-store --allow-anon
  pushing to ssh://user@dummy/repo
  searching for changes

Amend the bottom commit
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [9b5a54] A1
  $ echo 2 > A
  $ hg amend -qm A2 --rebase
  $ hg next
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [a24671] B1
  $ tglogp
  @  a24671c3bce2 draft 'B1'
  │
  o  a8543df036f1 draft 'A2'
  │
  o  df4f53cec30a public 'base'
  
  $ hgmn push ssh://user@dummy/repo -r . --bundle-store --allow-anon
  pushing to ssh://user@dummy/repo
  searching for changes
  $ hg debugmutation -r "draft()"
   *  a8543df036f16781d7f37d40d4f177056fc816a5 amend by test at 1970-01-01T00:00:00 from:
      9b5a540873ab29fbced488597365cf798918a356
  
   *  a24671c3bce21e759d256fe69dedeb04d51c9895 rebase by test at 1970-01-01T00:00:00 from:
      f99c737e05b52a0c08f95a8736581813ff58d8de
  
Pull the amended stack to the other repo
  $ cd $TESTTMP/repo-pull
  $ hgmn pull -r a24671c3bce2
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ tglogp
  o  a24671c3bce2 draft 'B1'
  │
  o  a8543df036f1 draft 'A2'
  │
  o  df4f53cec30a public 'base'
  

Check mutation metadata.
  $ hg debugmutation -r "draft()"
   *  a8543df036f16781d7f37d40d4f177056fc816a5 amend by test at 1970-01-01T00:00:00 from:
      9b5a540873ab29fbced488597365cf798918a356
  
   *  a24671c3bce21e759d256fe69dedeb04d51c9895 rebase by test at 1970-01-01T00:00:00 from:
      f99c737e05b52a0c08f95a8736581813ff58d8de
  
Amend the stack again.
  $ cd $TESTTMP/repo-push
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [a8543d] A2
  $ echo 3 > A
  $ hg amend -qm A3 --rebase
  $ hg next
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [647398] B1
  $ hgmn push ssh://user@dummy/repo -r . --bundle-store --allow-anon
  pushing to ssh://user@dummy/repo
  searching for changes

Pull the amended stack to the other repo.
  $ cd $TESTTMP/repo-pull
  $ hgmn pull -r 647398
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ tglogm
  o  6473983c899c 'B1'
  │
  o  5326b832c149 'A3'
  │
  │ x  a24671c3bce2 'B1'  (Rewritten using rebase into 6473983c899c)
  │ │
  │ x  a8543df036f1 'A2'  (Rewritten using amend into 5326b832c149)
  ├─╯
  o  df4f53cec30a 'base'
  

Do some more complicated mutations
  $ cd $TESTTMP/repo-push
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [5326b8] A3
  $ echo 1 > C
  $ hg commit -Aqm C1
  $ echo 2 > C
  $ hg amend -qm C2
  $ echo 3 > C
  $ hg amend -qm C3
  $ hg fold --from ".^"
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  rebasing 6473983c899c "B1"
  $ hg next
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [853e5b] B1
  $ tglogm
  @  853e5ba9bd35 'B1'
  │
  o  cdf849fe4126 'A3'
  │
  o  df4f53cec30a 'base'
  
  $ hgmn push ssh://user@dummy/repo -r . --bundle-store --allow-anon
  pushing to ssh://user@dummy/repo
  searching for changes

Pull the modified stack to the other repo.
  $ cd $TESTTMP/repo-pull
  $ hgmn pull -r 853e5ba9bd35
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ tglogm
  o  853e5ba9bd35 'B1'
  │
  o  cdf849fe4126 'A3'
  │
  │ x  6473983c899c 'B1'  (Rewritten using rebase into 853e5ba9bd35)
  │ │
  │ x  5326b832c149 'A3'  (Rewritten using fold into cdf849fe4126)
  ├─╯
  │ x  a24671c3bce2 'B1'  (Rewritten using rebase into 6473983c899c)
  │ │
  │ x  a8543df036f1 'A2'  (Rewritten using amend into 5326b832c149)
  ├─╯
  o  df4f53cec30a 'base'
  
