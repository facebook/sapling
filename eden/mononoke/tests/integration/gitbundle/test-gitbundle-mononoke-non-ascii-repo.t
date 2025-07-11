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
  $ echo "this is a file with UTF-8 content like ॐ" > file1
  $ git add file1
  $ git commit -q -am "मैं परीक्षण कर रहा हूँ"
  $ echo "this is another file with UTF-8 like नमस्ते" > file2
  $ git add file2
  $ git commit -q -am "यह एक और परीक्षा है"
  $ git tag -a empty_tag -m "टैग की गई प्रतिबद्धता"
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
  [INFO] GitRepo:$TESTTMP/repo-git commit 2 of 2 - Oid:d46416c1 => Bid:fa44539f
  [INFO] Hg: Sha1(65d52395d5907d3d7db131ac3a8d9e9e2a564009): HgManifestId(HgNodeHash(Sha1(6f475e77c1b34e29046bb4d7bbf8d181bb1db1a8)))
  [INFO] Hg: Sha1(d46416c10751191818e93a023d9d44db9ffd2513): HgManifestId(HgNodeHash(Sha1(f84a41aa4398ec5494ba1cdfbe1bdc65f29cd8ca)))
  [INFO] Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(fa44539fc506e5752424e2a05f43bccff5994cb860ac7a94351fb25baf2079ae)))
  [INFO] Ref: "refs/tags/empty_tag": Some(ChangesetId(Blake2(fa44539fc506e5752424e2a05f43bccff5994cb860ac7a94351fb25baf2079ae)))
  [INFO] Initializing repo: repo
  [INFO] Initialized repo: repo
  [INFO] All repos initialized. It took: * seconds (glob)
  [INFO] Bookmark: "heads/master_bookmark": ChangesetId(Blake2(fa44539fc506e5752424e2a05f43bccff5994cb860ac7a94351fb25baf2079ae)) (created)
  [INFO] Bookmark: "tags/empty_tag": ChangesetId(Blake2(fa44539fc506e5752424e2a05f43bccff5994cb860ac7a94351fb25baf2079ae)) (created)

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
  > d46416c mononoke यह एक और परीक्षा है HEAD -> master_bookmark, tag: empty_tag
  $TESTTMP.sh: line *: d46416c: command not found (glob)
  [127]

# Print out the log to ensure the commit messages have carried over without data loss
  $ git log --pretty=format:"%h %an %s %D"
  d46416c mononoke यह एक और परीक्षा है HEAD -> master_bookmark, tag: empty_tag
  65d5239 mononoke मैं परीक्षण कर रहा हूँ  (no-eol)

# Dump all the known Git objects into a file
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list

# Ensure that there are no differences between the set of objects by diffing both object list files
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list
