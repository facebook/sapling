# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hg init repo-hg --config format.usefncache=False

  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > commitextras=
  > treemanifest=
  > [treemanifest]
  > server=True
  > EOF

  $ touch file1
  $ hg add
  adding file1
  $ hg commit -m "adding commit with git metadata" --extra convert_revision=37b0a167e07f2b84149c918cec818ffeb183aaaa --extra hg-git-rename-source=git

  $ touch file2
  $ hg add
  adding file2
  $ hg commit -m "adding commit with git metadata" --extra convert_revision=37b0a167e07f2b84149c918cec818ffeb183bbbb --extra hg-git-rename-source=git

  $ touch file3
  $ hg add
  adding file3
  $ hg commit -m "no extras, expect to skip writing to bonsai_globalrev_mapping table"

  $ touch file4
  $ hg add
  adding file4
  $ hg commit -m "adding commit with git metadata" --extra convert_revision=37b0a167e07f2b84149c918cec818ffeb183dddd --extra hg-git-rename-source=git

  $ POPULATE_GIT_MAPPING=1 setup_mononoke_config
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo
  $ blobimport repo-hg/.hg repo > /dev/null
  [1]
  $ get_bonsai_git_mapping
  F81E4A4F6A773CFBCE1A4204B0A7BCA28EA200224AFDBC8C0FB47B5B42DFF249|37B0A167E07F2B84149C918CEC818FFEB183AAAA
  8B67890898056D938E08AD5025875C07B55EBE53CE8376C1106C5C8A1699D43D|37B0A167E07F2B84149C918CEC818FFEB183BBBB
  F2F8F29ECE7BD30C836C4949A7D2FEF10DEE3BC1B41C1200884C29DB05E0BD88|37B0A167E07F2B84149C918CEC818FFEB183DDDD

  $ cat "$TESTTMP/blobimport.out" | grep "git mapping"
  *] The git mapping is missing in bonsai commit extras: ChangesetId(Blake2(3a1bb821b2601e9da7300d0d56a88815c915c152fdbbce60fb38e22ecf99c293)) (glob)
  *] Error: Conflict detected while inserting git mappings (tried inserting: [BonsaiGitMappingEntry { git_sha1: GitSha1(37b0a167e07f2b84149c918cec818ffeb183aaaa), bcs_id: ChangesetId(Blake2(f81e4a4f6a773cfbce1a4204b0a7bca28ea200224afdbc8c0fb47b5b42dff249)) }, BonsaiGitMappingEntry { git_sha1: GitSha1(37b0a167e07f2b84149c918cec818ffeb183bbbb), bcs_id: ChangesetId(Blake2(8b67890898056d938e08ad5025875c07b55ebe53ce8376c1106c5c8a1699d43d)) }, BonsaiGitMappingEntry { git_sha1: GitSha1(37b0a167e07f2b84149c918cec818ffeb183dddd), bcs_id: ChangesetId(Blake2(f2f8f29ece7bd30c836c4949a7d2fef10dee3bc1b41c1200884c29db05e0bd88)) }]) (glob)
