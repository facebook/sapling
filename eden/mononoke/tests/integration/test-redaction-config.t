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

create master bookmark
  $ hg bookmark master_bookmark -r tip

create another commit that has other content we can redact
  $ echo c > c
  $ hg ci -A -q -m 'add c'
  $ hg bookmark other_bookmark -r tip

  $ hg log -T '{short(node)} {bookmarks}\n'
  7389ca641397 other_bookmark
  ac82d8b1f7c4 master_bookmark

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

  $ cd "$TESTTMP/repo-pull"
  $ hgmn pull -q
  $ hgmn up -q 14961831bd3a

Redact file 'c' in commit '7389ca6413976090442f3003d4329990bc688ef7'
  $ mononoke_newadmin redaction create-key-list -R repo -i 7389ca6413976090442f3003d4329990bc688ef7 c --main-bookmark master_bookmark --output-file rs_0
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  No files would be redacted in the main bookmark (master_bookmark)
  Redaction saved as: db4bf834eb70b32345de6a2ad146811a6d0591e24cc507b81e30070d01bf2798
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator

Attempt to redact file 'b' in commit '14961831bd3af3a6331fef7e63367d61cb6c9f6b'
This initially fails because it is still reachable in 'master'
  $ mononoke_newadmin redaction create-key-list -R repo -i 14961831bd3af3a6331fef7e63367d61cb6c9f6b b --main-bookmark master_bookmark
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  Redacted content in main bookmark: b content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9
  Error: Refusing to create key list because 1 files would be redacted in the main bookmark (master_bookmark)
  [1]

Try again with --force
  $ mononoke_newadmin redaction create-key-list -R repo -i 14961831bd3af3a6331fef7e63367d61cb6c9f6b b --main-bookmark master_bookmark --force --output-file rs_1
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  Redacted content in main bookmark: b content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9
  Creating key list despite 1 files being redacted in the main bookmark (master_bookmark) (--force)
  Redaction saved as: bd2b6b03fa8e5d9a9a68cf1cebc60b648d95b72781b9ada1debc57e4bba722f6
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator

  $ cat > "$REDACTION_CONF/redaction_sets" <<EOF
  > {
  >  "all_redactions": [
  >    {"reason": "T0", "id": "$(cat rs_0)", "enforce": false},
  >    {"reason": "T1", "id": "$(cat rs_1)", "enforce": true}
  >  ]
  > }
  > EOF
  $ rm rs_0 rs_1

The files should now be marked as redacted
  $ mononoke_newadmin redaction list -R repo -i 14961831bd3af3a6331fef7e63367d61cb6c9f6b
  Searching for redacted paths in c58e5684f660c327e9fd4cc0aba5e010bd444b0e0ee23fe4aa0cace2f44c0b46
  Found 1 redacted paths
  T1                  : b

  $ mononoke_newadmin redaction list -R repo -i 7389ca6413976090442f3003d4329990bc688ef7
  Searching for redacted paths in 39101456281e9b3d34041ded0c91b1712418c9eb59fbfc2bd06e873f3df9a6a4
  Found 1 redacted paths
  T0                  : c (log only)

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

Should gives us the tombstone file since it is redacted
  $ cat b
  This version of the file is redacted and you are not allowed to access it. Update or rebase to a newer commit.

Mononoke admin also won't give us the content
  $ mononoke_admin blobstore-fetch content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9
  *] using blobstore: Fileblob { base: "$TESTTMP/blobstore/blobs", put_behaviour: Overwrite } (glob)
  *] The blob content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9 is censored. (glob)
   Task/Sev: T1
  [1]

Same for newadmin
  $ mononoke_newadmin blobstore -R repo fetch content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9
  Error: Failed to fetch blob
  
  Caused by:
      The blob content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9 is censored.
       Task/Sev: T1
  [1]

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

Even is file b is redacted, we will get its content
  $ cat b
  b
