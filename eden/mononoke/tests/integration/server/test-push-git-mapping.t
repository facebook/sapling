# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' POPULATE_GIT_MAPPING=1 EMIT_OBSMARKERS=1 BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'
  $ hg up -q master_bookmark
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > infinitepush=
  > commitcloud=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF

Push first commit to infiniepush
  $ touch file1
  $ hg ci -Aqm commit1 --extra hg-git-rename-source=git --extra convert_revision=1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a
  $ hgmn push -q -r . --to "scratch/123" --create

Check that mappings are empty
  $ get_bonsai_git_mapping | sort

Push another commit to master
  $ touch file2
  $ hg ci -Aqm commit2 --extra hg-git-rename-source=git --extra convert_revision=2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b
  $ hgmn push -q -r . --to master_bookmark --create

Check that mappings were populated
  $ get_bonsai_git_mapping | sort
  3CEE0520D115C5973E538AFDEB6985C1DF2CFC2C8E58CE465B855D73993EFBA1|1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A
  E37E13B17B5C2B37965B2A9591A64CB2C44A68FD10F1362A595DA8C6E4EEFA41|2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B

Push a commit to infinitepush, then move bookmark to it
  $ touch file3
  $ hg ci -Aqm commit1 --extra hg-git-rename-source=git --extra convert_revision=3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c
  $ hgmn push -q -r . --to "scratch/123" --create

  $ get_bonsai_git_mapping | sort
  3CEE0520D115C5973E538AFDEB6985C1DF2CFC2C8E58CE465B855D73993EFBA1|1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A
  E37E13B17B5C2B37965B2A9591A64CB2C44A68FD10F1362A595DA8C6E4EEFA41|2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B

  $ hgmn push -q -r . --to "master_bookmark"
  $ get_bonsai_git_mapping | sort
  080A23640726489F849CF85B032DE1C47CBC78CE20A88F07F2DC031EBB8642FC|3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C
  3CEE0520D115C5973E538AFDEB6985C1DF2CFC2C8E58CE465B855D73993EFBA1|1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A
  E37E13B17B5C2B37965B2A9591A64CB2C44A68FD10F1362A595DA8C6E4EEFA41|2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B

Now push a commit to infinitepush, then force it to be public and then move bookmark to it
  $ touch file4
  $ hg ci -Aqm commit1 --extra hg-git-rename-source=git --extra convert_revision=4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d
  $ hgmn push -q -r . --to "scratch/123" --create

  $ hg log -r . -T '{node}\n' > "$TESTTMP"/commits_to_make_public
  $ mononoke_admin phases add-public "$TESTTMP"/commits_to_make_public &> /dev/null

  $ hgmn push -q -r . --to "master_bookmark"
  $ get_bonsai_git_mapping | sort
  080A23640726489F849CF85B032DE1C47CBC78CE20A88F07F2DC031EBB8642FC|3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C3C
  3CEE0520D115C5973E538AFDEB6985C1DF2CFC2C8E58CE465B855D73993EFBA1|1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A
  905E5F2B0B81809153B545F47E02487B48180303C857B9AF6B0DE0784D8C31DF|4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D4D
  E37E13B17B5C2B37965B2A9591A64CB2C44A68FD10F1362A595DA8C6E4EEFA41|2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B2B
