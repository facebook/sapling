# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Setup Mononoke (with the git population turned off)
  $ setup_common_config

# Test git mapping
  $ cd "$TESTTMP"

Setup testing repo for mononoke:
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server

Commit without git mapping:
  $ touch a
  $ hg add a
  $ hg commit -Am "first commit"
  $ export HG_HASH_1="$(hg --debug id -i)"

Commit git SHA:
  $ touch b
  $ hg add b
  $ hg commit -Am "commit with git sha" --extra convert_revision=37b0a167e07f2b84149c918cec818ffeb183dddd --extra hg-git-rename-source=git
  $ export HG_HASH_2="$(hg --debug id -i)"

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo --has-globalrev

  $ echo $HG_HASH_1 > hash_list
  $ echo $HG_HASH_2 > hash_list
  $ backfill_mapping --git hash_list

check that mapping is populated
  $ echo ${HG_HASH_1^^}
  D000F571737066778CC230F7DC9A763180FDE257
  $ echo ${HG_HASH_2^^}
  87B89069092550479FDF0EB22E632E031AF9C3D9

  $ get_bonsai_git_mapping
  1DE9FD24AAA4D21A00FF488B6C363C8E52CDFEC73E363766110A03C810821FEF|37B0A167E07F2B84149C918CEC818FFEB183DDDD

  $ get_bonsai_hg_mapping
  1DE9FD24AAA4D21A00FF488B6C363C8E52CDFEC73E363766110A03C810821FEF|87B89069092550479FDF0EB22E632E031AF9C3D9
  7BB4BC4B68FA09F86A9D757D345418ED6B83A1EF7FD6BF614FFA63F9338FBAC1|D000F571737066778CC230F7DC9A763180FDE257
