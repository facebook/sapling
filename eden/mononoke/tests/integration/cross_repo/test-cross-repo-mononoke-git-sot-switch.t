# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test both Mononoke Git and cross-repo infrastructure running together.
# Change source of truth and make sure commits can be synced from both ways, 
# with both sources of truths.

# Define the large and small repo ids and names before calling any helpers
  $ export LARGE_REPO_NAME="large_repo"
  $ export LARGE_REPO_ID=10
  $ export SUBMODULE_REPO_NAME="small_repo"
  $ export SUBMODULE_REPO_ID=11
  $ export SUBMODULE_REPO_GIT="$TESTTMP/small_repo_git"
  $ export MASTER_BOOKMARK_NAME="master"

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-git-submodule-expansion.sh"

Run the x-repo with submodules setup  
  $ ENABLE_API_WRITES=1 REPOID="$REPO_C_ID" REPONAME="repo_c" setup_common_config "$REPOTYPE"
  $ ENABLE_API_WRITES=1 REPOID="$REPO_B_ID" REPONAME="repo_b" setup_common_config "$REPOTYPE"

  $ run_common_xrepo_sync_with_gitsubmodules_setup
  L_A=b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4

  $ set_git_submodules_action_in_config_version "$LATEST_CONFIG_VERSION_NAME" "$SUBMODULE_REPO_ID" 3 # 3=expand
  $ set_git_submodule_dependencies_in_config_version "$LATEST_CONFIG_VERSION_NAME" \
  > "$SUBMODULE_REPO_ID" "{\"git-repo-b\": $REPO_B_ID, \"git-repo-b/git-repo-c\": $REPO_C_ID, \"repo_c\": $REPO_C_ID}"


# Setup git repos A, B and C
  $ setup_git_repos_a_b_c &> $TESTTMP/git_repos_setup.out

# Import all git repos into Mononoke
  $ gitimport_repos_a_b_c &> $TESTTMP/initial_gitimport.out

# Merge repo A into the large repo
  $ REPO_A_FOLDER="smallrepofolder1" merge_repo_a_to_large_repo &> $TESTTMP/merge_repos.out

# Set up live forward sync
  $ with_stripped_logs mononoke_x_repo_sync_forever "$SUBMODULE_REPO_ID" "$LARGE_REPO_ID"

# Start up the Mononoke Git Service
  $ MONONOKE_GIT_SERVICE_START_TIMEOUT=120 mononoke_git_service


# Clone the small repo from mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$SUBMODULE_REPO_NAME.git $SUBMODULE_REPO_GIT
  Cloning into '$TESTTMP/small_repo_git'...


# Make change to small repo
  $ cd $SUBMODULE_REPO_GIT || exit

  $ echo "new file" > added_from_git
  $ git add .
  $ git commit -q -am "Small repo commit after git clone" 

# TODO(T182967556): test pushing tags and other branches

# Push will fail because Mononoke Git is not source of truth
  $ git_client push origin --all --follow-tags
  error: unable to parse remote unpack status: Push rejected: Mononoke is not the source of truth for repo small_repo
  To https://localhost:$LOCAL_PORT/repos/git/ro/small_repo.git
   ! [remote rejected] master -> master (Push rejected: Mononoke is not the source of truth for repo small_repo)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/small_repo.git'
  [1]

# Set Mononoke Git as the Source of Truth
  $ REPO_ID=$SUBMODULE_REPO_ID REPONAME=$SUBMODULE_REPO_NAME  set_mononoke_as_source_of_truth_for_git 
  $ REPO_ID=$REPO_B_ID REPONAME="repo_b" set_mononoke_as_source_of_truth_for_git
  $ REPO_ID=$REPO_C_ID REPONAME="repo_c" set_mononoke_as_source_of_truth_for_git

# Now push succeeds
  $ git_client push origin --all --follow-tags
  To https://localhost:$LOCAL_PORT/repos/git/ro/small_repo.git
     3a41dad..c88eb10  master -> master

  $ with_stripped_logs wait_for_xrepo_sync 2

  $ export REPONAME=$LARGE_REPO_NAME
  $ cd $TESTTMP/$LARGE_REPO_NAME || exit
  $ hg pull -q
  $ hg checkout $MASTER_BOOKMARK_NAME -q

  $ sl_log -r "sort(all(), desc)" -l 3
  @  bee3089ac10c Small repo commit after git clone
  │
  o  e2b260a2b04f Added git repo C as submodule directly in A
  │
  o    c0240984981f [MEGAREPO GRADUAL MERGE] gradual merge (7)
  ├─╮
  │ │
  ~ ~
  $ switch_source_of_truth_to_large_repo $SUBMODULE_REPO_ID $LARGE_REPO_ID
  
  
  NOTE: Enable push redirection for small repo
  
  
  NOTE: Get current large repo bookmark update log id to set the backsyncer counter
  LARGE_REPO_BOOKMARK_UPDATE_LOG_ID: 11
  
  
  NOTE: Delete forward syncer counter and set backsyncer counter
  BACKSYNC_COUNTER: 11

  $ PREV_BOOK_VALUE=$(get_bookmark_value_bonsai "$SUBMODULE_REPO_NAME" "heads/master")
  $ echo "$PREV_BOOK_VALUE"
  8f67a0aced3d7aafbba86c1b1510fb7aa2bba7fde303fa27da272543c2341fd1

# Start backsyncer in the background
  $ REPOIDSMALL=$SUBMODULE_REPO_ID REPOIDLARGE=$LARGE_REPO_ID \
  > with_stripped_logs backsync_large_to_small_forever


# Make changes to small repo from the large repo  
  $ echo "change" > smallrepofolder1/from_large_repo
  $ mkdir forbidden
  $ echo "change" > forbidden/not_allowed
  $ hg commit -qA -m "Change submodule repo from large repo" 

# Make a change only to large repo to confirm it won't be backsynced
  $ echo "change" > large_repo_file.txt
  $ hg commit -qAm "Large repo file change" 


# Push to large repo, which should be backsynced to small repo
  $ hg push -q --to master


  $ mononoke_newadmin fetch -R $LARGE_REPO_NAME -B $MASTER_BOOKMARK_NAME
  BonsaiChangesetId: d8fafbaf8fd49e67ad90778c8fe5d8efaa1b5f8ed146f8471bcf688bdae52e91
  Author: test
  Message: Large repo file change
  FileChanges:
  	 ADDED/MODIFIED: large_repo_file.txt 7e0269c3137ea814b84ca0f2d4896f0cbc5e6216362803d4df88cf3f80536f0c
  

  $ ATTEMPTS=20 wait_for_bookmark_move_away_bonsai "$SUBMODULE_REPO_NAME" "heads/master" "$PREV_BOOK_VALUE"

  $ mononoke_newadmin fetch -R $SUBMODULE_REPO_NAME -B heads/master
  BonsaiChangesetId: 13715279cffa4966ef3572ed60d1779e42a103911c360618611a36b1c08ecd2e
  Author: test
  Message: Change submodule repo from large repo
  FileChanges:
  	 ADDED/MODIFIED: from_large_repo 7e0269c3137ea814b84ca0f2d4896f0cbc5e6216362803d4df88cf3f80536f0c
  

# NOTE: If I don't manually derive the git object, git pull gets a 500 error code.
# Mononoke Git logs show this: P1535517841
  $ mononoke_newadmin convert -R $SUBMODULE_REPO_NAME --derive \
  > -f bonsai -t git 13715279cffa4966ef3572ed60d1779e42a103911c360618611a36b1c08ecd2e
  b0bf2974fb9bfd512e54939869465847f49f9131

# Pull changes in the git repo to show that the commit was synced
  $ cd $SUBMODULE_REPO_GIT || exit
  $ git_client pull -q --rebase
  $ git_client log --oneline --no-abbrev-commit
  b0bf2974fb9bfd512e54939869465847f49f9131 Change submodule repo from large repo
  c88eb109cbc424f0b594f7bb199756aecb489681 Small repo commit after git clone
  3a41dad928c497de6a34dd21856dfdb9301f22fc Added git repo C as submodule directly in A
  f3ce0eec860e1f3f6abe4612daab1f5566964c29 Added git repo B as submodule in A
  ad7b60659402144a23adc0b21c2cdf196a90b012 Add regular_dir/aardvar
  8c33a271a275b1f84bcde8cd0cb6a55c71b5edae Add root_file

# Push a commit to Mononoke Git, which should be push redirected, now that the
# source of truth is the large repo.
  $ echo "another_from_git" > another_from_git
  $ git add .
  $ git commit -q -am "Git commit that should be pushredirected" 
  $ git_client push origin --all --follow-tags
  To https://localhost:$LOCAL_PORT/repos/git/ro/small_repo.git
   ! [remote rejected] master -> master (Submodule expansion data not provided when submodules is enabled for small repo)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/small_repo.git'
  [1]
