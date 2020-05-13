# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ cd $TESTTMP
  $ cat > $TESTTMP/mononoke_tunables.json <<EOF
  > {
  >   "killswitches": {
  >     "mutation_advertise_for_infinitepush": true,
  >     "mutation_accept_for_infinitepush": true,
  >     "mutation_generate_for_draft": true
  >   }
  > }
  > EOF

setup common configuration for these tests
  $ enable amend infinitepush commitcloud
  $ setconfig ui.ssh="\"$DUMMYSSH\""
  $ setconfig mutation.date="0 0" mutation.enabled=true mutation.record=true visibility.enabled=true
  $ setconfig experimental.evolution=obsolete

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch base
  $ hg commit -Aqm base
  $ tglogp
  @  0: df4f53cec30a draft 'base'
  

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
  @  2: f99c737e05b5 draft 'B1'
  |
  o  1: 9b5a540873ab draft 'A1'
  |
  o  0: df4f53cec30a public 'base' master_bookmark
  
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
  @  4: a24671c3bce2 draft 'B1'
  |
  o  3: a8543df036f1 draft 'A2'
  |
  o  0: df4f53cec30a public 'base' master_bookmark
  
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
  added 2 changesets with 0 changes to 0 files
  $ tglogp
  o  2: a24671c3bce2 draft 'B1'
  |
  o  1: a8543df036f1 draft 'A2'
  |
  o  0: df4f53cec30a public 'base' master_bookmark
  

Check mutation metadata.  NOTE: Mutation metadata hasn't been provided by the server.
  $ hg debugmutation -r "draft()"
   *  a8543df036f16781d7f37d40d4f177056fc816a5
  
   *  a24671c3bce21e759d256fe69dedeb04d51c9895
  
