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
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ quiet dd if=/dev/zero of=large_file bs=1M count=10
  $ git add .
  $ git commit -qam "Added file1 and large_file"
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ mkdir -p d1/d2/d3/d4/d5 && echo "this is file that is 5 levels deep" > d1/d2/d3/d4/d5/deep_file
  $ git add .
  $ git commit -qam "Add file2 and deep file"
  $ git tag -a empty_tag -m ""
  $ echo "this is modified large file" > large_file
  $ git add .
  $ git commit -qam "Modified large file"

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Partial clone the repo where the only allowed object type is blob (commits and tags are always included)
  $ cd "$TESTTMP"
  $ git clone --filter=object:type=blob --no-checkout file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...

# Get the count of objects received as part of this clone. Use count-objects instead of rev-list to prevent Git from downloading missing objects
# from remote since this is a partial clone
  $ cd $GIT_REPO
  $ git count-objects -v | grep "in-pack"
  in-pack: 10

# Partial clone the repo from Mononoke and ensure we get the same number of objects
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --filter=object:type=blob --no-checkout
  Cloning into 'repo'...

# Get the count of objects received as part of this clone. Use count-objects instead of rev-list to prevent Git from downloading missing objects
# from remote since this is a partial clone
  $ cd $REPONAME
  $ git count-objects -v | grep "in-pack"
  in-pack: 10

  $ cd "$TESTTMP"
  $ rm -rf $GIT_REPO
  $ rm -rf $REPONAME

# Another way of having blobless clone is with --filter=blob:none
  $ git clone --filter=blob:none --no-checkout file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...

# Get the count of objects received as part of this clone. Use count-objects instead of rev-list to prevent Git from downloading missing objects
# from remote since this is a partial clone
  $ cd $GIT_REPO
  $ git count-objects -v | grep "in-pack"
  in-pack: 13

# Partial clone the repo from Mononoke and ensure we get the same number of objects
  $ git_client clone --filter=blob:none --no-checkout $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...

# Get the count of objects received as part of this clone. Use count-objects instead of rev-list to prevent Git from downloading missing objects
# from remote since this is a partial clone
  $ cd $REPONAME
  $ git count-objects -v | grep "in-pack"
  in-pack: 13

  $ cd "$TESTTMP"
  $ rm -rf $GIT_REPO
  $ rm -rf $REPONAME

# Perform partial clone by filtering blobs >= 9MB (i.e. large_file file in this case)
  $ git clone --filter=blob:limit=9m --no-checkout file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...

# Get the count of objects received as part of this clone. Use count-objects instead of rev-list to prevent Git from downloading missing objects
# from remote since this is a partial clone
  $ cd $GIT_REPO
  $ git count-objects -v | grep "in-pack"
  in-pack: 17

# Partial clone the repo from Mononoke and ensure we get the same number of objects
  $ git_client clone --filter=blob:limit=9m --no-checkout $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...

# Get the count of objects received as part of this clone. Use count-objects instead of rev-list to prevent Git from downloading missing objects
# from remote since this is a partial clone
  $ cd $REPONAME
  $ git count-objects -v | grep "in-pack"
  in-pack: 17

# Using rev-list we can validate the lazy on-demand download of objects by Git works for partial repos
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Do the same for the Mononoke Git repo
  $ cd $REPONAME  
  $ git_client rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list

# Validate that after downloading all the required objects, we have the same state of repo in both cases
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list  
