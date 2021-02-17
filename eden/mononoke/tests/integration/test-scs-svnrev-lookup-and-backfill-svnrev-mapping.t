# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setup_common_config
  $ setup_configerator_configs
  $ cd "$TESTTMP"

Setup testing repo for mononoke:
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server

Commit without svn mapping:
  $ touch a
  $ hg add a
  $ hg commit -Am "first commit"
  $ export HG_HASH_1="$(hg --debug id -i)"

Commit a commit mapped from SVN:
  $ touch b
  $ hg add b
  $ hg commit -Am "commit with svnrev" --extra convert_revision=svn:22222222-aaaa-0000-aaaa-ddddddddcccc/repo/trunk/project@2077
  $ export HG_HASH_2="$(hg --debug id -i)"

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start SCS server
  $ start_and_wait_for_scs_server --scuba-log-file "$TESTTMP/scuba.json"

  $ echo $HG_HASH_1 > hash_list
  $ echo $HG_HASH_2 > hash_list
  $ backfill_mapping --svnrev hash_list
  * using repo "repo" repoid RepositoryId(0) (glob)

check that mapping is populated
  $ echo ${HG_HASH_1^^}
  D000F571737066778CC230F7DC9A763180FDE257
  $ echo ${HG_HASH_2^^}
  8369D94764C293BAABD6DEF07F48E4613B60F3BE

  $ get_bonsai_svnrev_mapping
  0949D5C4F49C53A89816984E6E32D35012DF772DDD451C4B9CD7B16F2908A65D|2077

  $ get_bonsai_hg_mapping
  0949D5C4F49C53A89816984E6E32D35012DF772DDD451C4B9CD7B16F2908A65D|8369D94764C293BAABD6DEF07F48E4613B60F3BE
  7BB4BC4B68FA09F86A9D757D345418ED6B83A1EF7FD6BF614FFA63F9338FBAC1|D000F571737066778CC230F7DC9A763180FDE257

lookup using bookmark
  $ scsc lookup --repo repo --hg-commit-id $HG_HASH_1 -S svnrev
  error: hg commit id 'd000f571737066778cc230f7dc9a763180fde257' does not have a 'svnrev' id
  [1]
  $ scsc lookup --repo repo --hg-commit-id $HG_HASH_2 -S svnrev
  2077

  $ scsc lookup --repo repo --svnrev 2077 -S hg
  8369d94764c293baabd6def07f48e4613b60f3be
