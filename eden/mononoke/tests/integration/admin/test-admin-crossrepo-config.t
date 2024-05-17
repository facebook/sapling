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
  TEST_VERSION_NAME
  TEST_VERSION_NAME_COMPLEX
  TEST_VERSION_NAME_OLD

  $ REPOID=0 mononoke_admin crossrepo config list --with-contents
  TEST_VERSION_NAME:
    large repo: 0
    common pushrebase bookmarks: [* "master_bookmark" *] (glob)
    version name: TEST_VERSION_NAME
      small repo: 1
      default action: Preserve
      prefix map:
        arvr->.fbsource-rest/arvr
      small repo: 2
      default action: PrependPrefix(NonRootMPath("arvr-legacy"))
      prefix map:
        arvr->arvr
        fbandroid->.ovrsource-rest/fbandroid
        fbcode->.ovrsource-rest/fbcode
        fbobjc->.ovrsource-rest/fbobjc
        xplat->.ovrsource-rest/xplat
  
  
  TEST_VERSION_NAME_COMPLEX:
    large repo: 0
    common pushrebase bookmarks: [* "master_bookmark" *] (glob)
    version name: TEST_VERSION_NAME_COMPLEX
      small repo: 1
      default action: Preserve
      prefix map:
        a/b/c1->ma/b/c1
        a/b/c2->ma/b/c2
        arvr->.fbsource-rest/arvr
        d/e->ma/b/c2/d/e
      small repo: 2
      default action: PrependPrefix(NonRootMPath("arvr-legacy"))
      prefix map:
        a/b/c1->ma/b/c1
        a/b/c2->ma/b/c2
        arvr->arvr
        d/e->ma/b/c2/d/e
        fbandroid->.ovrsource-rest/fbandroid
        fbcode->.ovrsource-rest/fbcode
        fbobjc->.ovrsource-rest/fbobjc
        xplat->.ovrsource-rest/xplat
  
  
  TEST_VERSION_NAME_OLD:
    large repo: 0
    common pushrebase bookmarks: [* "master_bookmark" *] (glob)
    version name: TEST_VERSION_NAME_OLD
      small repo: 1
      default action: Preserve
      prefix map:
        arvr->.fbsource-rest/arvr_old
      small repo: 2
      default action: PrependPrefix(NonRootMPath("arvr-legacy"))
      prefix map:
        arvr->arvr
        fbandroid->.ovrsource-rest/fbandroid
        fbcode->.ovrsource-rest/fbcode_old
        fbobjc->.ovrsource-rest/fbobjc
        xplat->.ovrsource-rest/xplat
  
  
  $ REPOID=0 mononoke_admin crossrepo config by-version TEST_VERSION_NAME_OLD
  large repo: 0
  common pushrebase bookmarks: [* "master_bookmark" *] (glob)
  version name: TEST_VERSION_NAME_OLD
    small repo: 1
    default action: Preserve
    prefix map:
      arvr->.fbsource-rest/arvr_old
    small repo: 2
    default action: PrependPrefix(NonRootMPath("arvr-legacy"))
    prefix map:
      arvr->arvr
      fbandroid->.ovrsource-rest/fbandroid
      fbcode->.ovrsource-rest/fbcode_old
      fbobjc->.ovrsource-rest/fbobjc
      xplat->.ovrsource-rest/xplat
