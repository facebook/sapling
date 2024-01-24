# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

Setup repositories
  $ REPOTYPE="blob_files"
  $ MEG_REPOID=0
  $ FBS_REPOID=1
  $ OVR_REPOID=2

  $ NO_BOOKMARKS_CACHE=1 REPOID=$MEG_REPOID REPONAME=meg-mon setup_common_config $REPOTYPE
  $ NO_BOOKMARKS_CACHE=1 REPOID=$FBS_REPOID REPONAME=fbs-mon setup_common_config $REPOTYPE
  $ NO_BOOKMARKS_CACHE=1 REPOID=$OVR_REPOID REPONAME=ovr-mon setup_common_config $REPOTYPE

  $ setup_commitsyncmap
  $ setup_configerator_configs
-- initial push-redirection setup redirects ovrsource into megarepo,
-- which is the large repo at this point
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "2": {
  >      "draft_push": false,
  >      "public_push": true
  >    }
  >   }
  > }
  > EOF

  $ testtool_drawdag -R fbs-mon --no-default-files --print-hg-hashes <<'EOF'
  > FBS_C
  >     |    # modify: FBS_C a/b/c2/f C
  >     |    # modify: FBS_C a/b/c1/f C
  >     |    # modify: FBS_C d/e C
  > FBS_B
  >     |    # modify: FBS_B otherfile_fbsource B
  > FBS_A
  >     |    # modify: FBS_A arvr/arvrfile_fbsource A
  >     |    # modify: FBS_A fbcode/fbcodefile_fbsource A
  >     |    # modify: FBS_A otherfile_fbsource A
  > FBS_EMPTY
  > EOF
  FBS_A=0c9bfef46e1767e657ddea77619d43b601eb49bc
  FBS_B=00750ef1c727947db19d4df77f7c121624541f1f
  FBS_C=fbc63a08295003d3238a8ac8914a0bb07034ab04
  FBS_EMPTY=cfc1c4bc16b3bbd4a5ca92803972051a778ff2b8

  $ testtool_drawdag -R meg-mon --no-default-files --print-hg-hashes <<'EOF'
  > MEG_H
  > |      # modify: MEG_H ma/b H
  > | MEG_G
  > | |    # modify: MEG_G ma/b/c2/d/e C
  > | MEG_F
  > | |    # modify: MEG_F ma/b/c2/d/e F
  > | MEG_E
  >  \|    # modify: MEG_E ma/b/c1/f C
  > \ |    # modify: MEG_E ma/b/c2/f C
  >   MEG_D
  >   |    # delete: MEG_D a/b/c1/f
  >   |    # delete: MEG_D a/b/c2/f
  >   |    # delete: MEG_D d/e
  >   MEG_C
  >   |    # modify: MEG_C a/b/c1/f C
  >   |    # modify: MEG_C a/b/c2/f C
  >   |    # modify: MEG_C d/e C
  >   |    # modify: MEG_A arvr-legacy/otherfile_ovrsource C
  >   MEG_B
  >   |    # modify: MEG_B otherfile_fbsource B
  >   MEG_A
  >   |    # modify: MEG_A otherfile_fbsource A
  >   |    # modify: MEG_A .fbsource-rest/arvr/arvrfile_fbsource A
  >   |    # modify: MEG_A .ovrsource-rest/fbcode/fbcodefile_ovrsource A
  >   |    # modify: MEG_A arvr-legacy/Research/researchfile_ovrsource A
  >   |    # modify: MEG_A arvr-legacy/otherfile_ovrsource A
  >   |    # modify: MEG_A arvr/arvrfile_ovrsource A
  >   |    # modify: MEG_A fbcode/fbcodefile_fbsource A
  >   MEG_EMPTY
  > EOF
  MEG_A=9397c93fdcef2f64d5352516c8fdcc957e9063d9
  MEG_B=7142d3c8005f98db26d9ca2acdc604a4958e367a
  MEG_C=a51c7e0c34be45e41af7b4153e1b86fcbaf89d69
  MEG_D=19c46e9699e09e6e8cc3e8ca2a46c50ada6e1fdb
  MEG_E=9b32bd1c994199b5326b9336645361dc731f4c19
  MEG_EMPTY=453f9eeced357a8ddaed6011d3aaa825381a947a
  MEG_F=9c22e766c41af9d8f14d838d36582a6aa8370e4d
  MEG_G=9ca2657dfbbeb2b2651ca0ce59c5107eff80716f
  MEG_H=fcaddd7112ceee0713a8c4ebe9f2ddd2201b216b

  $ testtool_drawdag -R ovr-mon --no-default-files --print-hg-hashes <<'EOF'
  > OVR_H
  > |      # modify: OVR_H a/b H
  > | OVR_G
  > | |    # modify: OVR_G d/e C
  > | OVR_F
  > | |    # modify: OVR_F d/e F
  > | OVR_E
  > \ |    # modify: OVR_E a/b/c2/f C
  >  \|    # modify: OVR_E a/b/c1/f C
  >   OVR_D
  >   |
  >   OVR_C
  >   |    # modify: OVR_A otherfile_ovrsource A
  >   OVR_B
  >   |
  >   OVR_A
  >   |    # modify: OVR_A fbcode/fbcodefile_ovrsource A
  >   |    # modify: OVR_A arvr/arvrfile_ovrsource A
  >   |    # modify: OVR_A otherfile_ovrsource A
  >   |    # modify: OVR_A Research/researchfile_ovrsource A
  >   OVR_EMPTY
  > EOF
  OVR_A=4aed0ec56ea76f0189b8a69522ecf54e8d2150f2
  OVR_B=4db034ef782c0bb6072b8694f55b1c0301ac1913
  OVR_C=c6a6159232b9befdb2f89b6bf378def35676667e
  OVR_D=743d53acdf575d02df8c701790f21b2e4f2eb1e8
  OVR_E=39df6c998e2aaa85468311d704bc3ec7ac1d151c
  OVR_EMPTY=f7d1dbe4729be8146c1bb38974b3fbb6bd91dd8d
  OVR_F=177f4b77e53e9add1ca1a24c9eae319d0a5ee62d
  OVR_G=c74fec0e6d164db791624ad8b2bf0b04d058367a
  OVR_H=c63a7f0861cd5c933e43e53318c7ad9ac8ee24e6

PART ONE: testing meg-mon <-> fbs-mon mapping verification
 * is default_action=preserve
 * we're mostly testing for discrepancies in large repo

Version A and B should be good wrt to TEST_VERSION_NAME and COMPLEX
  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $MEG_REPOID --target-repo-id $FBS_REPOID check-push-redirection-prereqs $MEG_A $FBS_A TEST_VERSION_NAME
  * all is well! (glob)

  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $MEG_REPOID --target-repo-id $FBS_REPOID check-push-redirection-prereqs $MEG_A $FBS_A TEST_VERSION_NAME_COMPLEX
  * all is well! (glob)

  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $MEG_REPOID --target-repo-id $FBS_REPOID check-push-redirection-prereqs $MEG_B $FBS_B TEST_VERSION_NAME
  * all is well! (glob)

  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $MEG_REPOID --target-repo-id $FBS_REPOID check-push-redirection-prereqs $MEG_B $FBS_B TEST_VERSION_NAME_COMPLEX
  * all is well! (glob)

Same for version C with TEST_VERSION_NAME
  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $MEG_REPOID --target-repo-id $FBS_REPOID check-push-redirection-prereqs $MEG_C $FBS_C TEST_VERSION_NAME
  * all is well! (glob)

With COMPLEX mappping mulitiple paths are in wrong place (but the tool fails on first)
  $ EXPECTED_RC=1 quiet_grep "NonRootMPath" -- megarepo_tool_multirepo --source-repo-id $MEG_REPOID --target-repo-id $FBS_REPOID check-push-redirection-prereqs $MEG_C $FBS_C TEST_VERSION_NAME_COMPLEX | sort
  Some(NonRootMPath("ma/b/c1/f")) is present in meg-mon, but not in fbs-mon (under Some(NonRootMPath("a/b/c1/f")))
  Some(NonRootMPath("ma/b/c2/d/e")) is present in meg-mon, but not in fbs-mon (under Some(NonRootMPath("d/e")))
  Some(NonRootMPath("ma/b/c2/f")) is present in meg-mon, but not in fbs-mon (under Some(NonRootMPath("a/b/c2/f")))

In version E one file is missing
  $ EXPECTED_RC=1 quiet_grep "NonRootMPath"  -- megarepo_tool_multirepo --source-repo-id $MEG_REPOID --target-repo-id $FBS_REPOID check-push-redirection-prereqs $MEG_E $FBS_C TEST_VERSION_NAME_COMPLEX
  Some(NonRootMPath("ma/b/c2/d/e")) is present in meg-mon, but not in fbs-mon (under Some(NonRootMPath("d/e")))
  [1]

In version F the file is present but the contents are wrong
  $ EXPECTED_RC=1 quiet_grep "NonRootMPath" -- megarepo_tool_multirepo --source-repo-id $MEG_REPOID --target-repo-id $FBS_REPOID check-push-redirection-prereqs $MEG_F $FBS_C TEST_VERSION_NAME_COMPLEX
  file differs between meg-mon (path: Some(NonRootMPath("ma/b/c2/d/e")), content_id: ContentId(Blake2(e6a553abb176f0acef0936e5d6e7930c8a590a62c07984bee8e3a8d5a2bb2ff9)), type: Regular) and fbs-mon (path: Some(NonRootMPath("d/e")), content_id: ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d)), type: Regular)
  file differs between meg-mon (path: Some(NonRootMPath("ma/b/c2/d/e")), content_id: ContentId(Blake2(e6a553abb176f0acef0936e5d6e7930c8a590a62c07984bee8e3a8d5a2bb2ff9)), type: Regular) and fbs-mon (path: Some(NonRootMPath("d/e")), content_id: ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d)), type: Regular)
  [1]

Version G is good
  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $MEG_REPOID --target-repo-id $FBS_REPOID check-push-redirection-prereqs $MEG_G $FBS_C TEST_VERSION_NAME_COMPLEX
  * all is well! (glob)

Version H has file vs directory conflict
  $ EXPECTED_RC=1 quiet_grep "NonRootMPath" -- megarepo_tool_multirepo --source-repo-id $MEG_REPOID --target-repo-id $FBS_REPOID check-push-redirection-prereqs $MEG_H $FBS_C TEST_VERSION_NAME_COMPLEX
  Some(NonRootMPath("ma/b")) is present in fbs-mon, but not in meg-mon (under Some(NonRootMPath("ma/b")))
  [1]

PART TWO: testing meg-mon <-> ovr-mon mapping verification
 * is default_action=prepend_prefix
 * we're mostly testing for discrepancies in small repo

Version A and B should be good wrt to TEST_VERSION_NAME and COMPLEX
  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $OVR_REPOID --target-repo-id $MEG_REPOID check-push-redirection-prereqs $OVR_A $MEG_A TEST_VERSION_NAME
  * all is well! (glob)

  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $OVR_REPOID --target-repo-id $MEG_REPOID check-push-redirection-prereqs $OVR_A $MEG_A TEST_VERSION_NAME_COMPLEX
  * all is well! (glob)

  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $OVR_REPOID --target-repo-id $MEG_REPOID check-push-redirection-prereqs $OVR_B $MEG_B TEST_VERSION_NAME
  * all is well! (glob)

  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $OVR_REPOID --target-repo-id $MEG_REPOID check-push-redirection-prereqs $OVR_B $MEG_B TEST_VERSION_NAME_COMPLEX
  * all is well! (glob)

Same for version C
  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $OVR_REPOID --target-repo-id $MEG_REPOID check-push-redirection-prereqs $OVR_C $MEG_G TEST_VERSION_NAME
  * all is well! (glob)

  $ EXPECTED_RC=1 quiet_grep "NonRootMPath" -- megarepo_tool_multirepo --source-repo-id $OVR_REPOID --target-repo-id $MEG_REPOID check-push-redirection-prereqs $OVR_C $MEG_G TEST_VERSION_NAME_COMPLEX | sort
  Some(NonRootMPath("a/b/c1/f")) is present in ovr-mon, but not in meg-mon (under Some(NonRootMPath("ma/b/c1/f")))
  Some(NonRootMPath("a/b/c2/f")) is present in ovr-mon, but not in meg-mon (under Some(NonRootMPath("ma/b/c2/f")))
  Some(NonRootMPath("d/e")) is present in ovr-mon, but not in meg-mon (under Some(NonRootMPath("ma/b/c2/d/e")))

In version E one file is missing
  $ EXPECTED_RC=1 quiet_grep "NonRootMPath"  -- megarepo_tool_multirepo --source-repo-id $OVR_REPOID --target-repo-id $MEG_REPOID check-push-redirection-prereqs $OVR_E $MEG_G TEST_VERSION_NAME_COMPLEX
  Some(NonRootMPath("d/e")) is present in ovr-mon, but not in meg-mon (under Some(NonRootMPath("ma/b/c2/d/e")))
  [1]

In version F the file is present but the contents are wrong
  $ EXPECTED_RC=1 quiet_grep "NonRootMPath" -- megarepo_tool_multirepo --source-repo-id $OVR_REPOID --target-repo-id $MEG_REPOID check-push-redirection-prereqs $OVR_F $MEG_G TEST_VERSION_NAME_COMPLEX
  file differs between meg-mon (path: Some(NonRootMPath("ma/b/c2/d/e")), content_id: ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d)), type: Regular) and ovr-mon (path: Some(NonRootMPath("d/e")), content_id: ContentId(Blake2(e6a553abb176f0acef0936e5d6e7930c8a590a62c07984bee8e3a8d5a2bb2ff9)), type: Regular)
  [1]

Version G is good
  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $OVR_REPOID --target-repo-id $MEG_REPOID check-push-redirection-prereqs $OVR_G $MEG_G TEST_VERSION_NAME_COMPLEX
  * all is well! (glob)

Version H has file vs directory conflict
  $ EXPECTED_RC=1 quiet_grep "NonRootMPath" -- megarepo_tool_multirepo --source-repo-id $OVR_REPOID --target-repo-id $MEG_REPOID check-push-redirection-prereqs $OVR_H $MEG_G TEST_VERSION_NAME_COMPLEX | sort
  Some(NonRootMPath("a/b/c1/f")) is present in ovr-mon, but not in meg-mon (under Some(NonRootMPath("ma/b/c1/f")))
  Some(NonRootMPath("a/b/c2/f")) is present in ovr-mon, but not in meg-mon (under Some(NonRootMPath("ma/b/c2/f")))
  Some(NonRootMPath("d/e")) is present in ovr-mon, but not in meg-mon (under Some(NonRootMPath("ma/b/c2/d/e")))
