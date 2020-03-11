# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup configuration
  $ BLOB_TYPE="blob_files" MONONOKE_HGCLI_PRIORITY=wishlist quiet default_setup

Check that priority is passed over
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  remote: Using priority: Wishlist
  searching for changes
  no changes found
  adding changesets
  devel-warn: * (glob)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
