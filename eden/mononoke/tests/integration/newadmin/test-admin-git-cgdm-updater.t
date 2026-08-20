# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

# Setup Mononoke config with preloaded_cgdm_blobstore_key
  $ setup_common_config "blob_files"
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<EOF
  > [git_configs]
  > preloaded_cgdm_blobstore_key = "test_cgdm_key"
  > EOF
  $ cd "$TESTTMP"

# Create a git repo with several commits
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ mkdir -p "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "file1 content" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ echo "file2 content" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ echo "file3 content" > file3
  $ git add file3
  $ git commit -qam "Add file3"

# Import into Mononoke with bookmarks
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --generate-bookmarks full-repo

# Test single-repo mode: update CGDM with explicit blobstore key
# Shared args (--component-max-count, --component-max-size) come before the subcommand
  $ mononoke_admin git-cgdm-updater --component-max-count 100 --component-max-size 10485760 repo -R repo --all-bookmarks --blobstore-key manual_cgdm_key
  [repo] Loading existing CGDMComponents from mutable blobstore key 'manual_cgdm_key'
  [repo] Mutable blobstore lookup completed in * (miss) (glob)
  [repo] Loading existing CGDMComponents from immutable blobstore key 'manual_cgdm_key'
  [repo] Immutable blobstore lookup completed in * (miss) (glob)
  [repo] Loaded 0 existing components with 0 changesets (rebuild: false)
  [repo] Found 3 new changesets to process
  [repo] Finished calculating GDM sizes
  [repo] Finished assigning changesets to 1 components
  [repo] Storing CGDM blobs for 0 new full components
  [repo] Saving updated CGDMComponents to blobstore key 'manual_cgdm_key'
  [repo] CGDM update complete

# Verify components were created via git-cgdm-components
  $ mononoke_admin git-cgdm-components -R repo --blobstore-key manual_cgdm_key | wc -l
  * (glob)

# Test all-repos mode: discovers repo from config and uses all bookmarks
  $ mononoke_admin git-cgdm-updater --component-max-count 100 --component-max-size 10485760 all-repos
  Found * repos with preloaded_cgdm_blobstore_key configured (glob)
  [INFO] Initializing repo: repo
  [INFO] Initialized repo: repo (1/1)
  [INFO] All repos initialized. It took: * seconds (glob)
  Updating CGDM for repo repo with blobstore key test_cgdm_key
  [repo] Listing publishing bookmarks
  [repo] Listed * publishing bookmarks in * (glob)
  [repo] Loading existing CGDMComponents from mutable blobstore key 'test_cgdm_key'
  [repo] Mutable blobstore lookup completed in * (miss) (glob)
  [repo] Loading existing CGDMComponents from immutable blobstore key 'test_cgdm_key'
  [repo] Immutable blobstore lookup completed in * (miss) (glob)
  [repo] Loaded 0 existing components with 0 changesets (rebuild: false)
  [repo] Found 3 new changesets to process
  [repo] Finished calculating GDM sizes
  [repo] Finished assigning changesets to 1 components
  [repo] Storing CGDM blobs for 0 new full components
  [repo] Saving updated CGDMComponents to blobstore key 'test_cgdm_key'
  [repo] CGDM update complete

# Verify components were created at config-derived key
  $ mononoke_admin git-cgdm-components -R repo --blobstore-key test_cgdm_key | wc -l
  * (glob)

# Test all-repos mode with --rebuild flag
  $ mononoke_admin git-cgdm-updater --component-max-count 100 --component-max-size 10485760 --rebuild all-repos
  Found * repos with preloaded_cgdm_blobstore_key configured (glob)
  [INFO] Initializing repo: repo
  [INFO] Initialized repo: repo (1/1)
  [INFO] All repos initialized. It took: * seconds (glob)
  Updating CGDM for repo repo with blobstore key test_cgdm_key
  [repo] Listing publishing bookmarks
  [repo] Listed * publishing bookmarks in * (glob)
  [repo] Loaded 0 existing components with 0 changesets (rebuild: true)
  [repo] Found 3 new changesets to process
  [repo] Finished calculating GDM sizes
  [repo] Finished assigning changesets to 1 components
  [repo] Storing CGDM blobs for 0 new full components
  [repo] Saving updated CGDMComponents to blobstore key 'test_cgdm_key'
  [repo] CGDM update complete
