# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOTYPE="blob_files"
  $ REPOID=0 REPONAME=repo setup_common_config $REPOTYPE
  $ REPOID=1 REPONAME=repo1 setup_common_config $REPOTYPE
  $ REPOID=2 REPONAME=repo2 setup_common_config $REPOTYPE
  $ setup_commitsyncmap

  $ cd $TESTTMP

setup hg server repo

  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo --no-default-files <<'EOF'
  > FBSOURCE OVRSOURCE
  > # modify: FBSOURCE "fbcode/fbcodfile_fbsource" "fbcode/fbcodfile_fbsource\n"
  > # modify: FBSOURCE "fbobjc/fbobjcfile_fbsource" "fbobjc/fbobjcfile_fbsource\n"
  > # modify: FBSOURCE "fbandroid/fbandroidfile_fbsource" "fbandroid/fbandroidfile_fbsource\n"
  > # modify: FBSOURCE "xplat/xplatfile_fbsource" "xplat/xplatfile_fbsource\n"
  > # modify: FBSOURCE "arvr/arvrfile_fbsource" "arvr/arvrfile_fbsource\n"
  > # modify: FBSOURCE "third-party/thirdpartyfile_fbsource" "third-party/thirdpartyfile_fbsource\n"
  > # modify: OVRSOURCE "fbcode/fbcodfile_ovrsource" "fbcode/fbcodfile_ovrsource\n"
  > # modify: OVRSOURCE "fbobjc/fbobjcfile_ovrsource" "fbobjc/fbobjcfile_ovrsource\n"
  > # modify: OVRSOURCE "fbandroid/fbandroidfile_ovrsource" "fbandroid/fbandroidfile_ovrsource\n"
  > # modify: OVRSOURCE "xplat/xplatfile_ovrsource" "xplat/xplatfile_ovrsource\n"
  > # modify: OVRSOURCE "arvr/arvrfile_ovrsource" "arvr/arvrfile_ovrsource\n"
  > # modify: OVRSOURCE "third-party/thirdpartyfile_ovrsource" "third-party/thirdpartyfile_ovrsource\n"
  > # modify: OVRSOURCE "Software/softwarefile_ovrsource" "Software/softwarefile_ovrsource\n"
  > # modify: OVRSOURCE "Research/researchfile_ovrsource" "Research/researchfile_ovrsource\n"
  > # bookmark: FBSOURCE fbsource_master
  > # bookmark: OVRSOURCE ovrsource_master
  > # message: FBSOURCE "fbsource-like commit"
  > # message: OVRSOURCE "ovrsource-like commit"
  > EOF
  FBSOURCE=ced1efc1c752e00f6a984bb92a598d23aedffd8d3dbf6a8adc83692cd31bf373
  OVRSOURCE=23227cbe43072a39ad47a537a879ffadc63dce19c3ce9ad2cedbb8f4d34e04b1
  $ cd $TESTTMP

setup repo-pull
  $ hg clone -q mono:repo repo-pull --noupdate

  $ export COMMIT_DATE="1985-09-04T00:00:00.00Z"
move things in fbsource
  $ RUST_BACKTRACE=1 mononoke_admin megarepo move-commit --repo-id 0 \
  > --source-repo-id 1 -B fbsource_master -a user -m "fbsource move" \
  > --mark-public --commit-date-rfc3339 "$COMMIT_DATE" \
  > --set-bookmark fbsource_move --mapping-version-name TEST_VERSION_NAME
  * Marked as public * (glob)
  * Setting bookmark * "fbsource_move" * to point to * (glob)
  * Setting bookmark * "fbsource_move" * finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of *: HgChangesetId(HgNodeHash(Sha1(*))) (glob)

move things in ovrsource in a stack
  $ mononoke_admin megarepo move-commit --repo-id 0 --source-repo-id 2 \
  > -B ovrsource_master -a user -m "ovrsource stack move" --mark-public \
  >  --commit-date-rfc3339 "$COMMIT_DATE" --max-num-of-moves-in-commit 1 \
  > --set-bookmark ovrsource_move --mapping-version-name TEST_VERSION_NAME
  * Marked as public * (glob)
  * Setting bookmark * "ovrsource_move" * to point to * (glob)
  * Setting bookmark * "ovrsource_move" * finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark * "ovrsource_move" * to point to * (glob)
  * Setting bookmark * "ovrsource_move" * finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark * "ovrsource_move" * to point to * (glob)
  * Setting bookmark * "ovrsource_move" * finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark * "ovrsource_move" * to point to * (glob)
  * Setting bookmark * "ovrsource_move" * finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark * "ovrsource_move" * to point to * (glob)
  * Setting bookmark * "ovrsource_move" * finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark * "ovrsource_move" * to point to * (glob)
  * Setting bookmark * "ovrsource_move" * finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark * "ovrsource_move" * to point to * (glob)
  * Setting bookmark * "ovrsource_move" * finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * created 7 commits, with the last commit * (glob)

merge things in both repos
  $ mononoke_admin megarepo merge --repo-id 0 -B fbsource_move  \
  > -B ovrsource_move -a user -m "megarepo merge" --mark-public  \
  > --commit-date-rfc3339 "$COMMIT_DATE" --set-bookmark master_bookmark
  * Creating a merge commit (glob)
  * Checking if there are any path conflicts (glob)
  * Done checking path conflicts (glob)
  * Creating a merge bonsai changeset with parents: * (glob)
  * Marked as public * (glob)
  * Setting bookmark * "master_bookmark" * to point to * (glob)
  * Setting bookmark * "master_bookmark" * finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of *: HgChangesetId(HgNodeHash(Sha1(*))) (glob)

start mononoke server
  $ start_and_wait_for_mononoke_server
pull the result
  $ cd $TESTTMP/repo-pull
  $ hg -q pull && hg -q up master_bookmark
  $ ls -1
  arvr
  arvr-legacy
  fbandroid
  fbcode
  fbobjc
  third-party
  xplat
  $ ls -1 fbcode fbandroid fbobjc xplat arvr arvr-legacy
  arvr:
  arvrfile_ovrsource
  
  arvr-legacy:
  Research
  Software
  third-party
  
  fbandroid:
  fbandroidfile_fbsource
  
  fbcode:
  fbcodfile_fbsource
  
  fbobjc:
  fbobjcfile_fbsource
  
  xplat:
  xplatfile_fbsource

test pre-merge deletes functionality
  $ cd "$TESTTMP"
  $ mononoke_admin megarepo pre-merge-delete --repo-id 0 \
  > --bookmark master_bookmark \
  > -a author -m "merge preparation" \
  > --even-chunk-size 4 \
  > --commit-date-rfc3339 "$COMMIT_DATE" 2>/dev/null
  3c56085394e5ca1d5c0fc66cdbfbb8f05d1cb0b7a6ba2868f41cb698a732553c
  302cbf62158949d2bddac21ba30d7aba01d54df2aecc66f68f3ba4d7a0431ea5
  91e46871162377faa9026e39d9c90fb371e2c5953864198dfb834e14104b7e31

test gradual-delete functionality
  $ LAST_DELETION_BONSAI=$(mononoke_admin megarepo gradual-delete --repo-id 0 \
  > --bookmark master_bookmark \
  > -a author -m "gradual deletion" \
  > --even-chunk-size 1 \
  > --commit-date-rfc3339 "$COMMIT_DATE" \
  > arvr fbcode 2>/dev/null | tail -1)
  $ echo $LAST_DELETION_BONSAI
  8f64fd6997e30133216e1f03bc803959ee49bb5cca98caf234c7fb7fcd287376
  $ LAST_DELETION_COMMIT=$(mononoke_admin convert -R repo -f bonsai -t hg --derive $LAST_DELETION_BONSAI)
  $ echo $LAST_DELETION_COMMIT
  4b384e4a818424eab83bc86e547bad5f9183459b

  $ cd $TESTTMP/repo-pull
  $ sl pull -q -r $LAST_DELETION_COMMIT
  $ sl log --stat -r "reverse($LAST_DELETION_COMMIT % master_bookmark)"
  commit:      4b384e4a8184
  user:        author
  date:        Wed Sep 04 00:00:00 1985 +0000
  summary:     [MEGAREPO DELETE] gradual deletion (1)
  
   fbcode/fbcodfile_fbsource |  1 -
   1 files changed, 0 insertions(+), 1 deletions(-)
  
  commit:      92dbf1c8c06c
  user:        author
  date:        Wed Sep 04 00:00:00 1985 +0000
  summary:     [MEGAREPO DELETE] gradual deletion (0)
  
   arvr/arvrfile_ovrsource |  1 -
   1 files changed, 0 insertions(+), 1 deletions(-)
  
  $ sl files -r "$LAST_DELETION_COMMIT" arvr fbcode || echo "Directories have been deleted"
  Directories have been deleted

run mover
  $ mononoke_admin megarepo run-mover \
  > --source-repo-id 0 --target-repo-id 1 \
  > --version TEST_VERSION_NAME --path foo/bar
  Ok(Some(NonRootMPath("foo/bar")))
  $ mononoke_admin megarepo run-mover \
  > --source-repo-id 0 --target-repo-id 2 \
  > --version TEST_VERSION_NAME --path arvr-legacy/foo
  Ok(Some(NonRootMPath("foo")))
  $ mononoke_admin megarepo run-mover \
  > --source-repo-id 2 --target-repo-id 0 \
  > --version TEST_VERSION_NAME --path foo/bar
  Ok(Some(NonRootMPath("arvr-legacy/foo/bar")))
