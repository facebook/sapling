# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration
  $ MONONOKE_DIRECT_PEER=1
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

  $ cd $TESTTMP

setup hg server repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg ci -A -q -m 'add a'

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-pull and repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate

blobimport
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > rebase =
  > remotenames =
  > EOF

  $ cd ../repo-push

  $ hgmn up -q 0
Push files
  $ echo b > b
  $ echo f > f

  $ mkdir dir
  $ mkdir dir/dirdir
  $ echo 'c' > dir/c
  $ echo 'd' > dir/d
  $ echo 'g' > dir/g
  $ echo 'e' > dir/dirdir/e
  $ hg ci -A -q -m "add b,c,d and e"

  $ hgmn push -q -r .  --to master_bookmark

  $ tglogpnr
  @  2cc2702dde1d public 'add b,c,d and e'  default/master_bookmark
  │
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

Censor file (file 'b' in commit '2cc2702dde1d7133c30a1ed763ee82c04befb237')
  $ mononoke_admin redaction add "[TASK]Censor b" 2cc2702dde1d7133c30a1ed763ee82c04befb237 b --force --log-only
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  $ mononoke_admin redaction list 2cc2702dde1d7133c30a1ed763ee82c04befb237
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Listing redacted files for ChangesetId: HgChangesetId(HgNodeHash(Sha1(*))) (glob)
  * Please be patient. (glob)
  * [TASK]Censor b      : b (log only) (glob)
  $ mononoke_admin redaction add "[TASK]Censor b" 2cc2702dde1d7133c30a1ed763ee82c04befb237 b --force
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  $ mononoke_admin redaction list 2cc2702dde1d7133c30a1ed763ee82c04befb237
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Listing redacted files for ChangesetId: HgChangesetId(HgNodeHash(Sha1(*))) (glob)
  * Please be patient. (glob)
  * [TASK]Censor b      : b (glob)
