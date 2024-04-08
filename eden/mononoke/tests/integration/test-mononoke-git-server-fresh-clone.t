# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests", "unodes", "filenodes", "hgchangesets"]' setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Capture the current bonsai_git_mapping output representing the git commits generated for the bonsais
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT hex(git_sha1) as git_id, hex(bcs_id) as bonsai_id FROM bonsai_git_mapping ORDER BY hex(git_sha1)" > bonsai_git_mapping

# Delete all the imported git commit mappings so that we are forced to regenerate them from bonsais
# instead of reusing the raw git commit from the original git repo
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM bonsai_git_mapping"
  $ find -type f -name '*git_object*' -delete

# Regenerate git commits and trees
  $ quiet backfill_derived_data backfill-all git_trees git_commits git_delta_manifests unodes

# Capture the new bonsai_git_mapping output representing the git commits re-generated for the bonsais
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT hex(git_sha1) as git_id, hex(bcs_id) as bonsai_id FROM bonsai_git_mapping ORDER BY hex(git_sha1)" > new_bonsai_git_mapping

# Ensure that the git commits generated are the same as the ones directly imported from git
  $ diff -w $TESTTMP/new_bonsai_git_mapping $TESTTMP/bonsai_git_mapping

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...
# Verify that we get the same Git repo back that we started with
  $ cd $REPONAME  
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list
