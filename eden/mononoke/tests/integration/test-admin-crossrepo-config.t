# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configerator configs
  $ setup_mononoke_config
  $ setup_configerator_configs

test various admin commands
  $ REPOID=0 mononoke_admin crossrepo config list
  * Initializing CfgrLiveCommitSyncConfig (glob)
  * Done initializing CfgrLiveCommitSyncConfig (glob)
  TEST_VERSION_NAME
  TEST_VERSION_NAME_OLD

  $ REPOID=0 mononoke_admin crossrepo config list --with-contents
  * Initializing CfgrLiveCommitSyncConfig (glob)
  * Done initializing CfgrLiveCommitSyncConfig (glob)
  TEST_VERSION_NAME:
    large repo: 0
    common pushrebase bookmarks: [BookmarkName { bookmark: "master_bookmark" }]
    version name: TEST_VERSION_NAME
      small repo: 1
      default action: Preserve
      prefix map:
        arvr->.fbsource-rest/arvr
      small repo: 2
      default action: PrependPrefix(MPath("arvr-legacy"))
      prefix map:
        arvr->arvr
        fbandroid->.ovrsource-rest/fbandroid
        fbcode->.ovrsource-rest/fbcode
        fbobjc->.ovrsource-rest/fbobjc
        xplat->.ovrsource-rest/xplat
  
  
  TEST_VERSION_NAME_OLD:
    large repo: 0
    common pushrebase bookmarks: [BookmarkName { bookmark: "master_bookmark" }]
    version name: TEST_VERSION_NAME_OLD
      small repo: 1
      default action: Preserve
      prefix map:
        arvr->.fbsource-rest/arvr_old
      small repo: 2
      default action: PrependPrefix(MPath("arvr-legacy"))
      prefix map:
        arvr->arvr
        fbandroid->.ovrsource-rest/fbandroid
        fbcode->.ovrsource-rest/fbcode_old
        fbobjc->.ovrsource-rest/fbobjc
        xplat->.ovrsource-rest/xplat
  
  
  $ REPOID=0 mononoke_admin crossrepo config by-version TEST_VERSION_NAME_OLD
  * Initializing CfgrLiveCommitSyncConfig (glob)
  * Done initializing CfgrLiveCommitSyncConfig (glob)
  large repo: 0
  common pushrebase bookmarks: [BookmarkName { bookmark: "master_bookmark" }]
  version name: TEST_VERSION_NAME_OLD
    small repo: 1
    default action: Preserve
    prefix map:
      arvr->.fbsource-rest/arvr_old
    small repo: 2
    default action: PrependPrefix(MPath("arvr-legacy"))
    prefix map:
      arvr->arvr
      fbandroid->.ovrsource-rest/fbandroid
      fbcode->.ovrsource-rest/fbcode_old
      fbobjc->.ovrsource-rest/fbobjc
      xplat->.ovrsource-rest/xplat
