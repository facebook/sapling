# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration

  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

  $ cd $TESTTMP

setup hg server repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg ci -A -q -m 'add a'

  $ hg log -T '{short(node)}\n'
  ac82d8b1f7c4

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-pull and repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull2 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull3 --noupdate

blobimport
  $ blobimport repo-hg/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > rebase =
  > remotenames =
  > EOF

  $ cd ../repo-pull
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

  $ cd ../repo-pull2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

  $ cd ../repo-pull3
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

  $ cd ../repo-push

  $ hgmn up -q 0
  $ echo b > b
  $ hg ci -A -q -m "add b"

  $ hgmn push -q -r .  --to master_bookmark

  $ hg log -T '{node}\n'
  14961831bd3af3a6331fef7e63367d61cb6c9f6b
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90

  $ cd "$TESTTMP/repo-pull"
  $ hgmn pull -q
  $ hgmn up -q 14961831bd3a

Censor the redacted blob (file 'b' in commit '14961831bd3af3a6331fef7e63367d61cb6c9f6b')
  $ MONONOKE_EXEC_STAGE=admin mononoke_admin redaction create-key-list 14961831bd3af3a6331fef7e63367d61cb6c9f6b b --force | head -n 1 | sed 's/Redaction saved as: //g' > rs_1
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * changeset resolved as: * (glob)
  $ cat > "$REDACTION_CONF/redaction_sets" <<EOF
  > {
  >  "all_redactions": [
  >    {"reason": "T1", "id": "$(cat rs_1)", "enforce": true}
  >  ]
  > }
  > EOF
  $ rm rs_1

# We could not restart mononoke here, but then we'd have to wait 60s for it to
# update the redaction config automatically
Restart mononoke
  $ killandwait $MONONOKE_PID
  $ rm -rf "$TESTTMP/mononoke-config"
  $ setup_common_config blob_files
  $ mononoke
  $ wait_for_mononoke

  $ cd "$TESTTMP/repo-pull2"
  $ hgmn pull -q
  $ hgmn up -q 14961831bd3a

  $ tglogpnr
  @  14961831bd3a public 'add b'  default/master_bookmark
  │
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

Should gives us the tombstone file since it is redacted
  $ cat b
  This version of the file is redacted and you are not allowed to access it. Update or rebase to a newer commit.

Restart mononoke and disable redaction verification
  $ killandwait $MONONOKE_PID
  $ rm -rf "$TESTTMP/mononoke-config"
  $ export REDACTION_DISABLED=1
  $ setup_common_config blob_files
  $ mononoke
  $ wait_for_mononoke

  $ cd "$TESTTMP/repo-pull3"
  $ hgmn pull -q
  $ hgmn up -q 14961831bd3a

  $ tglogpnr
  @  14961831bd3a public 'add b'  default/master_bookmark
  │
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  

Even is file b is redacted, we will get its content
  $ cat b
  b
