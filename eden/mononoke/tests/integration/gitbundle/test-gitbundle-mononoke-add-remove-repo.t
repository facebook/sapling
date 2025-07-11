# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo"
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q

# Add FileA with some content and commit it
  $ echo "This is a file that will be later deleted in a future commit" > FileA
  $ git add FileA
  $ git commit -q -am "Add FileA"
  $ git tag -a -m "First commit tag" first_tag

# Add FileB with different content and commit it
  $ echo "This is a file whose content will change in a future commit" > FileB
  $ git add FileB
  $ git commit -q -am "Add FileB"
  $ git tag -a empty_tag -m ""

# Create a file with exact same contents as an earlier file but in a different directory
  $ mkdir -p src/path/to
  $ echo "This is a file that will be later deleted in a future commit" > src/path/to/FileA
  $ git add .
  $ git commit -q -am "Add src/path/to/FileB"

# Delete an existing file from the repository
  $ rm -rf FileA
  $ git add .
  $ git commit -q -am "Removed FileA from repo"

# Change the content of FileB
  $ echo "Changed FileB content" > FileB
  $ git add .
  $ git commit -q -am "Changed FileB"
  $ git tag -a changed_tag -m "Tag for change in FileB"

# Re-add the same FileA with the same content as before and return the content
# of FileB to be the same content as before
  $ echo "This is a file that will be later deleted in a future commit" > FileA
  $ echo "This is a file whose content will change in a future commit" > FileB
  $ git add .
  $ git commit -q -am "Brought FileA and FileB back to their original state"

# Clone the Git repo
  $ cd "$TESTTMP"
  $ git clone --mirror "$GIT_REPO_ORIGIN" repo-git
  Cloning into bare repository 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Get the repository log
  $ git log --pretty=format:"%h %an %s %D" > $TESTTMP/repo_log

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo
  [INFO] using repo "repo" repoid RepositoryId(0)
  [INFO] GitRepo:$TESTTMP/repo-git commit 6 of 6 - Oid:eaab34e2 => Bid:46aaf164
  [INFO] Hg: Sha1(2154e071b1a8791b06bffcc506b313732e647c94): HgManifestId(HgNodeHash(Sha1(8b4f4e3c7cdc2b7c79ac51132b5b21c2f6ca75f4)))
  [INFO] Hg: Sha1(7a943e3b6c349234007f438a42565a5c81461e34): HgManifestId(HgNodeHash(Sha1(9237cfb65969a500b246928029919ad9ba13cff8)))
  [INFO] Hg: Sha1(8cb9702418601aff8ba65807515b82d36eb89e6f): HgManifestId(HgNodeHash(Sha1(554a096671a16fa8fb94909fdbee4a41def86f87)))
  [INFO] Hg: Sha1(f2bf8c92a37c36ee07a9f6ba5bde3422e6d0788d): HgManifestId(HgNodeHash(Sha1(c64762ec4fd8e04369930aec2971803b2fa0475e)))
  [INFO] Hg: Sha1(5c645d3009bdeb8ac864092ef55b6f0e7ff80b20): HgManifestId(HgNodeHash(Sha1(92ccee2e879ae4a6f676af03c74248ff44b0e19e)))
  [INFO] Hg: Sha1(eaab34e23a4913154849dc575959af4b9e34952b): HgManifestId(HgNodeHash(Sha1(924c65491a4e3d620ca2aaee0ea6c5e6677a5dcf)))
  [INFO] Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(46aaf16419210c2dc75eac297cef4024ca5d27bc46a0d5ab3d2daf0d6759eba8)))
  [INFO] Ref: "refs/tags/changed_tag": Some(ChangesetId(Blake2(8f1100c7f9508d6a8c43e5694e3ecf7eb847930441d321285d8d219f37492a84)))
  [INFO] Ref: "refs/tags/empty_tag": Some(ChangesetId(Blake2(59c6bca772928f6e501b7417e06a21f12c9a685fdef67e565eceb8ad4800fa7f)))
  [INFO] Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(254b74066b04ddd9d5b235566a4c97c29244c689bc1fd37e8c528e26075383d7)))
  [INFO] Initializing repo: repo
  [INFO] Initialized repo: repo
  [INFO] All repos initialized. It took: * seconds (glob)
  [INFO] Bookmark: "heads/master_bookmark": ChangesetId(Blake2(46aaf16419210c2dc75eac297cef4024ca5d27bc46a0d5ab3d2daf0d6759eba8)) (created)
  [INFO] Bookmark: "tags/changed_tag": ChangesetId(Blake2(8f1100c7f9508d6a8c43e5694e3ecf7eb847930441d321285d8d219f37492a84)) (created)
  [INFO] Bookmark: "tags/empty_tag": ChangesetId(Blake2(59c6bca772928f6e501b7417e06a21f12c9a685fdef67e565eceb8ad4800fa7f)) (created)
  [INFO] Bookmark: "tags/first_tag": ChangesetId(Blake2(254b74066b04ddd9d5b235566a4c97c29244c689bc1fd37e8c528e26075383d7)) (created)

# Regenerate the Git repo out of the Mononoke repo
  $ mononoke_admin git-bundle create from-repo -R repo --output-location "$BUNDLE_PATH"
# Ensure that Git considers this a valid bundle
  $ cd $GIT_REPO
  $ git bundle verify -q $BUNDLE_PATH
  $TESTTMP/repo_bundle.bundle is okay

# Create a new empty folder for containing the repo
  $ mkdir $TESTTMP/git_client_repo
  $ cd "$TESTTMP"
  $ git clone --mirror "$BUNDLE_PATH" git_client_repo
  Cloning into bare repository 'git_client_repo'...
  $ cd git_client_repo

# Get the repository log and verify if its the same as earlier
  $ git log --pretty=format:"%h %an %s %D" > $TESTTMP/new_repo_log
  $ diff -w $TESTTMP/new_repo_log $TESTTMP/repo_log

# Dump all the known Git objects into a file
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list

# Ensure that there are no differences between the set of objects by diffing both object list files
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list
