# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd "$TESTTMP"
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ echo 1 > 1.sh
  $ hg add 1.sh
  $ hg commit -m first
  $ chmod +x 1.sh
  $ hg commit -m 'change mode'
  $ hg bookmark main -r .
  $ hg log -r ':' -T '{node}\n' > "$TESTTMP"/input_commits
  $ hg log -r 'tip' -T '{node}\n' > "$TESTTMP"/input_commits_latest
  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo

run validation
  $ backfill_derived_data --with-readonly-storage true validate  --backfill --parallel --batch-size=10 hgchangesets --input-file "$TESTTMP"/input_commits 2>&1 | grep 'Validation successful'
  * Validation successful!* (glob)
  $ backfill_derived_data --with-readonly-storage true validate  --backfill --parallel --batch-size=10 hgchangesets --input-file "$TESTTMP"/input_commits_latest 2>&1 | grep 'Validation successful'
  * Validation successful!* (glob)
