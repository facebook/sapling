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

setup repo with testtool_drawdag
  $ testtool_drawdag -R repo --no-default-files --derive-all --print-hg-hashes <<EOF
  > C
  > |
  > A
  > # modify: A "a" "a"
  > # modify: C "c" "c"
  > # bookmark: A master_bookmark
  > # bookmark: C other_bookmark
  > EOF
  A=* (glob)
  C=* (glob)

start mononoke
  $ start_and_wait_for_mononoke_server

setup repo-pull and repo-push
  $ hg clone -q mono:repo repo-push --noupdate
  $ hg clone -q mono:repo repo-pull --noupdate
  $ hg clone -q mono:repo repo-pull2 --noupdate
  $ hg clone -q mono:repo repo-pull3 --noupdate
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > rebase =
  > EOF

  $ cd ../repo-pull
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

  $ cd ../repo-pull2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

  $ cd ../repo-pull3
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

  $ cd ../repo-push

  $ hg up -q 0
  $ echo b > b
  $ hg ci -A -q -m "add b"

  $ hg push -q -r .  --to master_bookmark

  $ cd "$TESTTMP/repo-pull"
  $ hg pull -q
  $ COMMIT_B=$(hg log -r 'desc("add b")' -T '{node}')
  $ hg up -q $COMMIT_B

Redact file 'c' in commit '$C'
  $ mononoke_admin redaction create-key-list -R repo -i $C c --main-bookmark master_bookmark --output-file rs_0
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  No files would be redacted in the main bookmark (master_bookmark)
  Redaction saved as: * (glob)
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator

  $ mononoke_admin redaction fetch-key-list -R repo --output-file "$TESTTMP/keys" $(cat rs_0)
  $ cat "$TESTTMP/keys"
  content.blake2.000a1a9b74aa3da71fcceb653a62cb6987ae440c2b5c3d7e5d08d7c526b1dca8

Attempt to redact file 'b' in commit '$COMMIT_B'
This initially fails because it is still reachable in 'master'
  $ mononoke_admin redaction create-key-list -R repo -i $COMMIT_B b --main-bookmark master_bookmark
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  Redacted content in main bookmark: b content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9
  Error: Refusing to create key list because 1 files would be redacted in the main bookmark (master_bookmark)
  [1]

Try again with --force
  $ mononoke_admin redaction create-key-list -R repo -i $COMMIT_B b --main-bookmark master_bookmark --force --output-file rs_1
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  Redacted content in main bookmark: b content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9
  Creating key list despite 1 files being redacted in the main bookmark (master_bookmark) (--force)
  Redaction saved as: * (glob)
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
  $ mononoke_admin redaction list -R repo -i $COMMIT_B
  Searching for redacted paths in * (glob)
  Found 1 redacted paths
  T1                  : b

  $ mononoke_admin redaction list -R repo -i $C
  Searching for redacted paths in * (glob)
  Found 1 redacted paths
  T0                  : c (log only)

# We could not restart mononoke here, but then we'd have to wait 60s for it to
# update the redaction config automatically
Restart mononoke
  $ killandwait $MONONOKE_PID
  $ rm -rf "$TESTTMP/mononoke-config"
  $ setup_common_config blob_files
  $ start_and_wait_for_mononoke_server --enable-wbc-with no-derivation

  $ cd "$TESTTMP/repo-pull2"
# Don't share caches.
  $ setconfig remotefilelog.cachepath="$(pwd)/.hg/cache"
  $ hg pull -q
  $ hg up -q $COMMIT_B

Should gives us the tombstone file since it is redacted
  $ cat b
  This version of the file is redacted and you are not allowed to access it. Update or rebase to a newer commit.


Mononoke admin also won't give us the content
  $ mononoke_admin blobstore -R repo fetch content.blake2.21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9
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
# Don't share caches.
  $ setconfig remotefilelog.cachepath="$(pwd)/.hg/cache"
  $ hg pull -q
  $ hg up -q $COMMIT_B

Even is file b is redacted, we will get its content
  $ cat b
  b
