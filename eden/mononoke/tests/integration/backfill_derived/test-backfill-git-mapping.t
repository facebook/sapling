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
  $ testtool_drawdag --print-hg-hashes -R repo --derive-all --no-default-files <<EOF
  > A-B
  > # modify: A "a" "file_content"
  > # modify: B "b" "file_content"
  > # message: A "first commit"
  > # message: B "commit with git sha"
  > # extra: B  convert_revision "f350b5c6e32b47fa2fdfc104a6670436439110ec"
  > # extra: B  hg-git-rename-source "git"
  > EOF
  A=95cd11f31e0f6e7f36548cf49e032c87326e9c76
  B=833378ee4669a6bb8f1fbefa6d1a0c731b5ae31c

  $ echo $B > hash_list
  $ backfill_mapping --git hash_list

check that mapping is populated
  $ echo ${A^^}
  95CD11F31E0F6E7F36548CF49E032C87326E9C76
  $ echo ${B^^}
  833378EE4669A6BB8F1FBEFA6D1A0C731B5AE31C

  $ get_bonsai_git_mapping
  40F737643CE7257B8A32058DCAD01D2FAAB9B8F1EC9C6DA459033B367CB3036F|F350B5C6E32B47FA2FDFC104A6670436439110EC
  5813E88365587DA762E3FB9902F2128DAD1107CF33323C3E640D78700710E03B|A95913EE898E934E6FEA274872154D297A9F6869

  $ get_bonsai_hg_mapping
  40F737643CE7257B8A32058DCAD01D2FAAB9B8F1EC9C6DA459033B367CB3036F|833378EE4669A6BB8F1FBEFA6D1A0C731B5AE31C
  5813E88365587DA762E3FB9902F2128DAD1107CF33323C3E640D78700710E03B|95CD11F31E0F6E7F36548CF49E032C87326E9C76
