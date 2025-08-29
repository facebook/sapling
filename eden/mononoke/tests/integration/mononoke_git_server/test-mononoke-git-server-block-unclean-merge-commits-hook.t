# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO_SUBMODULE="${TESTTMP}/origin/repo-submodule-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

  $ cat >> repos/repo/server.toml <<EOF
  > [[bookmarks]]
  > regex=".*"
  > [[bookmarks.hooks]]
  > hook_name="block_unclean_merge_commits"
  > [[hooks]]
  > name="block_unclean_merge_commits"
  > config_json='''{
  >   "only_check_branches_matching_regex": "master_bookmark"
  > }'''
  > bypass_pushvar="x-git-allow-unclean-merges=1"
  > EOF

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:run_hooks_on_additional_changesets": true
  >   }
  > }
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Clone the Git repo from Mononoke
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git

# Enter the repo
  $ cd "${TESTTMP}/repo"

# Let us set up two divergent branches and push them
  $ createdivergentgitbranches branch1 master_bookmark file1
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     8ce3eae..8311103  master_bookmark -> master_bookmark
   * [new branch]      branch1 -> branch1

# Let us merge branch1 into master_bookmark and resolve merge conflicts
  $ git merge branch1
  Auto-merging file1
  CONFLICT (content): Merge conflict in file1
  Automatic merge failed; fix conflicts and then commit the result.
  [1]
  $ echo "This is file1 after merging branch1 into master_bookmark with resolved conflicts" > file1
  $ git commit -qam "Merge branch1 into master_bookmark"

# The merge was not "clean"
  $ git show --pretty='' . | wc
        8      36     239

# This is how the repo looks like
  $ showgitrepo
  *   8eced2e (HEAD -> master_bookmark) Merge branch1 into master_bookmark
  |\  
  | * 10b934f (origin/branch1, branch1) Changed file1 on branch1
  * | 8311103 (origin/master_bookmark, origin/HEAD) Changed file1 on master_bookmark
  |/  
  * 8ce3eae Add file1

# Push should fail
  $ git_client push origin master_bookmark
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    block_unclean_merge_commits for 8eced2e3eebf78d195405afdbe0257ff3796f1c2: The bookmark matching regex master_bookmark can't have merge commits with conflicts, even if they have been resolved
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# Let us test the bypass now
  $ git_client -c http.extraHeader="x-git-allow-unclean-merges: 1" push --all 
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     8311103..8eced2e  master_bookmark -> master_bookmark

# Let us point both branches at the same commit and push it to the server so client and the server have the same baseline again
  $ git checkout branch1
  Switched to branch 'branch1'
  $ git merge master_bookmark
  Updating 10b934f..8eced2e
  Fast-forward
   file1 | 2 +-
   1 file changed, 1 insertion(+), 1 deletion(-)
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     10b934f..8eced2e  branch1 -> branch1

# This is how the repo looks like
  $ showgitrepo
  *   8eced2e (HEAD -> branch1, origin/master_bookmark, origin/branch1, origin/HEAD, master_bookmark) Merge branch1 into master_bookmark
  |\  
  | * 10b934f Changed file1 on branch1
  * | 8311103 Changed file1 on master_bookmark
  |/  
  * 8ce3eae Add file1

# Let us set up two divergent branches and push them
  $ createdivergentgitbranches branch1 master_bookmark file1
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     8eced2e..97d653f  branch1 -> branch1
     8eced2e..ba3fec7  master_bookmark -> master_bookmark

# Let us merge master_bookmark into the branch the hook does not trigger on.
  $ git checkout branch1 -q
  $ git merge master_bookmark
  Auto-merging file1
  CONFLICT (content): Merge conflict in file1
  Automatic merge failed; fix conflicts and then commit the result.
  [1]
  $ echo "This is file1 after merging master_bookmark into branch1 resolving conflicts" > file1
  $ git commit -qam "Merge master_bookmark into branch1"

# This is how the repo looks like
  $ showgitrepo
  *   dd218d8 (HEAD -> branch1) Merge master_bookmark into branch1
  |\  
  | * ba3fec7 (origin/master_bookmark, origin/HEAD, master_bookmark) Changed file1 on master_bookmark
  * | 97d653f (origin/branch1) Changed file1 on branch1
  |/  
  *   8eced2e Merge branch1 into master_bookmark
  |\  
  | * 10b934f Changed file1 on branch1
  * | 8311103 Changed file1 on master_bookmark
  |/  
  * 8ce3eae Add file1

# The merge is unclean
  $ git show --pretty='' . | wc
        8      35     235

# Let us push everything up to this point.
# We are also pushing the commit that has an unclean merge but it is on a bookmark
# the hook does not run on because of the regex in the config so the push works.
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     97d653f..dd218d8  branch1 -> branch1

# Let us create one new commit on master_bookmark
  $ git checkout master_bookmark
  Switched to branch 'master_bookmark'
  Your branch is up to date with 'origin/master_bookmark'.
  $ echo "This is file2 on master_bookmark" > file2
  $ git add .
  $ git commit -qam "Changed file2 on master_bookmark"


# Let us create the clean merge commit on master_bookmark by merging branch1 into it
  $ git merge branch1
  Merge made by the 'ort' strategy.
   file1 | 2 +-
   1 file changed, 1 insertion(+), 1 deletion(-)

# This is how the repo looks like
  $ showgitrepo
  *   a53fce9 (HEAD -> master_bookmark) Merge branch 'branch1' into master_bookmark
  |\  
  | *   dd218d8 (origin/branch1, branch1) Merge master_bookmark into branch1
  | |\  
  | * | 97d653f Changed file1 on branch1
  * | | 884a03f Changed file2 on master_bookmark
  | |/  
  |/|   
  * | ba3fec7 (origin/master_bookmark, origin/HEAD) Changed file1 on master_bookmark
  |/  
  *   8eced2e Merge branch1 into master_bookmark
  |\  
  | * 10b934f Changed file1 on branch1
  * | 8311103 Changed file1 on master_bookmark
  |/  
  * 8ce3eae Add file1

# The merge is clean
  $ git show --pretty='' . | wc
        0       0       0

# Push should succeed as the merge is clean
  $ git_client push origin master_bookmark
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     ba3fec7..a53fce9  master_bookmark -> master_bookmark

# Set up new case with unclean merge and conflict resolved via deletion. The push
# should fail
  $ git_client -c http.extraHeader="x-git-allow-unclean-merges: 1" push --all
  Everything up-to-date
  $ git checkout branch1
  Switched to branch 'branch1'
  $ git merge master_bookmark
  Updating dd218d8..a53fce9
  Fast-forward
   file2 | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 file2
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     dd218d8..a53fce9  branch1 -> branch1

  $ createdivergentgitbranches branch1 master_bookmark file1
  $ git checkout master_bookmark -q
  $ git merge branch1
  Auto-merging file1
  CONFLICT (content): Merge conflict in file1
  Automatic merge failed; fix conflicts and then commit the result.
  [1]
  $ rm -rf file1
  $ git add .
  $ git commit -qam "Resolve merge by deleting file1"
  $ git show --pretty='' . | wc
        8      29     190
  $ git_client push origin master_bookmark
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    block_unclean_merge_commits for 5e502d552484171f94fcbafb17e029013ecabf33: The bookmark matching regex master_bookmark can't have merge commits with conflicts, even if they have been resolved
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# Set up new case when both parents introduced the same content, merge is clean and push succeeds
  $ git_client -c http.extraHeader="x-git-allow-unclean-merges: 1" push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     a53fce9..bb56732  branch1 -> branch1
     a53fce9..5e502d5  master_bookmark -> master_bookmark
  $ git checkout branch1
  Switched to branch 'branch1'
  $ git merge master_bookmark
  Updating bb56732..5e502d5
  Fast-forward
   file1 | 1 -
   1 file changed, 1 deletion(-)
   delete mode 100644 file1
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     bb56732..5e502d5  branch1 -> branch1
  $ createdivergentgitbranches branch1 master_bookmark file1 file1content
  $ git checkout master_bookmark -q
  $ git merge branch1
  Merge made by the 'ort' strategy.
  $ git show --pretty='' . | wc
        0       0       0
  $ git_client push origin master_bookmark
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     5e502d5..d657644  master_bookmark -> master_bookmark

# Set up new case with conflicted parents but conflict resolving data to the same parent. This should succeed.
  $ git_client -c http.extraHeader="x-git-allow-unclean-merges: 1" push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     5e502d5..0b37570  branch1 -> branch1
  $ git checkout branch1
  Switched to branch 'branch1'
  $ git merge master_bookmark
  Updating 0b37570..d657644
  Fast-forward
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     0b37570..d657644  branch1 -> branch1
  $ createdivergentgitbranches branch1 master_bookmark file1
  $ git checkout master_bookmark -q
  $ cp file1 "${TESTTMP}/.file1"
  $ git merge branch1
  Auto-merging file1
  CONFLICT (content): Merge conflict in file1
  Automatic merge failed; fix conflicts and then commit the result.
  [1]
  $ mv "${TESTTMP}/.file1" file1
  $ git add .
  $ git commit -qam "Resolve merge conflict by taking changes from one of the parents as they are"
  $ git show --pretty='' . | wc
        0       0       0
  $ git_client push origin master_bookmark
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     d657644..e6c1afb  master_bookmark -> master_bookmark

# Set up new case with clean merge but changes in non-overlapping hunks
  $ git_client -c http.extraHeader="x-git-allow-unclean-merges: 1" push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     d657644..fe275d7  branch1 -> branch1
  $ git checkout branch1
  Switched to branch 'branch1'
  $ git merge master_bookmark
  Updating fe275d7..e6c1afb
  Fast-forward
   file1 | 2 +-
   1 file changed, 1 insertion(+), 1 deletion(-)
  $ cat >file1 <<EOF
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > a
  > EOF
  $ git commit -qam "base commit"
  $ git checkout master_bookmark -q
  $ git merge branch1
  Updating e6c1afb..97532dd
  Fast-forward
   file1 | 43 ++++++++++++++++++++++++++++++++++++++++++-
   1 file changed, 42 insertions(+), 1 deletion(-)
  $ cp file1 "${TESTTMP}/.file1"
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     fe275d7..97532dd  branch1 -> branch1
     e6c1afb..97532dd  master_bookmark -> master_bookmark
  $ showgitrepo
  * 97532dd (HEAD -> master_bookmark, origin/master_bookmark, origin/branch1, origin/HEAD, branch1) base commit
  *   e6c1afb Resolve merge conflict by taking changes from one of the parents as they are
  |\  
  | * fe275d7 Changed file1 on branch1
  * | a8d85d3 Changed file1 on master_bookmark
  |/  
  *   d657644 Merge branch 'branch1' into master_bookmark
  |\  
  | * 0b37570 Changed file1 on branch1
  * | 9918ff7 Changed file1 on master_bookmark
  |/  
  *   5e502d5 Resolve merge by deleting file1
  |\  
  | * bb56732 Changed file1 on branch1
  * | 9d17996 Changed file1 on master_bookmark
  |/  
  *   a53fce9 Merge branch 'branch1' into master_bookmark
  |\  
  | *   dd218d8 Merge master_bookmark into branch1
  | |\  
  | * | 97d653f Changed file1 on branch1
  * | | 884a03f Changed file2 on master_bookmark
  | |/  
  |/|   
  * | ba3fec7 Changed file1 on master_bookmark
  |/  
  *   8eced2e Merge branch1 into master_bookmark
  |\  
  | * 10b934f Changed file1 on branch1
  * | 8311103 Changed file1 on master_bookmark
  |/  
  * 8ce3eae Add file1
  $ createdivergentgitbranches branch1 master_bookmark file1 file1content
  $ git checkout master_bookmark
  Already on 'master_bookmark'
  Your branch is ahead of 'origin/master_bookmark' by 1 commit.
    (use "git push" to publish your local commits)
  $ (cat "${TESTTMP}/.file1"; echo "last line") > file1
  $ git add .
  $ git commit --amend -m "master_bookmark commit"
  [master_bookmark 97232a1] master_bookmark commit
   Date: Sat Jan 1 00:00:00 2000 +0000
   1 file changed, 1 insertion(+)
  $ git checkout branch1
  Switched to branch 'branch1'
  $ (echo "first line"; cat "${TESTTMP}/.file1") > file1
  $ git add .
  $ git commit --amend -m "branch1 commit"
  [branch1 b73b0f5] branch1 commit
   Date: Sat Jan 1 00:00:00 2000 +0000
   1 file changed, 1 insertion(+)
  $ git checkout master_bookmark -q
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     97532dd..b73b0f5  branch1 -> branch1
     97532dd..97232a1  master_bookmark -> master_bookmark
  $ git merge branch1
  Auto-merging file1
  Merge made by the 'ort' strategy.
   file1 | 1 +
   1 file changed, 1 insertion(+)
  $ git show --pretty='' . | wc
        0       0       0
  $ showgitrepo
  *   db1eb8d (HEAD -> master_bookmark) Merge branch 'branch1' into master_bookmark
  |\  
  | * b73b0f5 (origin/branch1, branch1) branch1 commit
  * | 97232a1 (origin/master_bookmark, origin/HEAD) master_bookmark commit
  |/  
  * 97532dd base commit
  *   e6c1afb Resolve merge conflict by taking changes from one of the parents as they are
  |\  
  | * fe275d7 Changed file1 on branch1
  * | a8d85d3 Changed file1 on master_bookmark
  |/  
  *   d657644 Merge branch 'branch1' into master_bookmark
  |\  
  | * 0b37570 Changed file1 on branch1
  * | 9918ff7 Changed file1 on master_bookmark
  |/  
  *   5e502d5 Resolve merge by deleting file1
  |\  
  | * bb56732 Changed file1 on branch1
  * | 9d17996 Changed file1 on master_bookmark
  |/  
  *   a53fce9 Merge branch 'branch1' into master_bookmark
  |\  
  | *   dd218d8 Merge master_bookmark into branch1
  | |\  
  | * | 97d653f Changed file1 on branch1
  * | | 884a03f Changed file2 on master_bookmark
  | |/  
  |/|   
  * | ba3fec7 Changed file1 on master_bookmark
  |/  
  *   8eced2e Merge branch1 into master_bookmark
  |\  
  | * 10b934f Changed file1 on branch1
  * | 8311103 Changed file1 on master_bookmark
  |/  
  * 8ce3eae Add file1
# file1 is different in both parents but in non-overlapping hunks. Even thought git show is empty, we decided this should fail. 
  $ git_client push origin master_bookmark
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    block_unclean_merge_commits for db1eb8dcc8ac7919a7da931a9b527275472c96b7: The bookmark matching regex master_bookmark can't have merge commits with conflicts, even if they have been resolved
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]


# Case with deletion where in one parent the file is deleted and in the other it has not been touched since the LCA
  $ git_client -c http.extraHeader="x-git-allow-unclean-merges: 1" push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     97232a1..db1eb8d  master_bookmark -> master_bookmark
  $ git checkout branch1
  Switched to branch 'branch1'
  $ git merge master_bookmark
  Updating b73b0f5..db1eb8d
  Fast-forward
   file1 | 1 +
   1 file changed, 1 insertion(+)
  $ echo file1 > file1
  $ git commit -qam "base commit"
  $ git checkout master_bookmark -q
  $ git merge branch1
  Updating db1eb8d..b6b8fd0
  Fast-forward
   file1 | 45 +--------------------------------------------
   1 file changed, 1 insertion(+), 44 deletions(-)
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     b73b0f5..b6b8fd0  branch1 -> branch1
     db1eb8d..b6b8fd0  master_bookmark -> master_bookmark
  $ showgitrepo
  * b6b8fd0 (HEAD -> master_bookmark, origin/master_bookmark, origin/branch1, origin/HEAD, branch1) base commit
  *   db1eb8d Merge branch 'branch1' into master_bookmark
  |\  
  | * b73b0f5 branch1 commit
  * | 97232a1 master_bookmark commit
  |/  
  * 97532dd base commit
  *   e6c1afb Resolve merge conflict by taking changes from one of the parents as they are
  |\  
  | * fe275d7 Changed file1 on branch1
  * | a8d85d3 Changed file1 on master_bookmark
  |/  
  *   d657644 Merge branch 'branch1' into master_bookmark
  |\  
  | * 0b37570 Changed file1 on branch1
  * | 9918ff7 Changed file1 on master_bookmark
  |/  
  *   5e502d5 Resolve merge by deleting file1
  |\  
  | * bb56732 Changed file1 on branch1
  * | 9d17996 Changed file1 on master_bookmark
  |/  
  *   a53fce9 Merge branch 'branch1' into master_bookmark
  |\  
  | *   dd218d8 Merge master_bookmark into branch1
  | |\  
  | * | 97d653f Changed file1 on branch1
  * | | 884a03f Changed file2 on master_bookmark
  | |/  
  |/|   
  * | ba3fec7 Changed file1 on master_bookmark
  |/  
  *   8eced2e Merge branch1 into master_bookmark
  |\  
  | * 10b934f Changed file1 on branch1
  * | 8311103 Changed file1 on master_bookmark
  |/  
  * 8ce3eae Add file1
  $ createdivergentgitbranches branch1 master_bookmark file1
  $ git checkout master_bookmark -q
  $ git checkout HEAD~1 -- file1
  $ echo abcd > file3
  $ git add .
  $ git commit --amend -m "master_bookmark commit"
  [master_bookmark e1f85f9] master_bookmark commit
   Date: Sat Jan 1 00:00:00 2000 +0000
   1 file changed, 1 insertion(+)
   create mode 100644 file3
  $ git checkout branch1
  Switched to branch 'branch1'
  $ rm file1
  $ git add .
  $ git commit --amend -m "branch1 commit"
  [branch1 72c4b2c] branch1 commit
   Date: Sat Jan 1 00:00:00 2000 +0000
   1 file changed, 1 deletion(-)
   delete mode 100644 file1
  $ git checkout master_bookmark -q
  $ git_client push --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     b6b8fd0..72c4b2c  branch1 -> branch1
     b6b8fd0..e1f85f9  master_bookmark -> master_bookmark
  $ git merge branch1
  Merge made by the 'ort' strategy.
   file1 | 1 -
   1 file changed, 1 deletion(-)
   delete mode 100644 file1
  $ git show --pretty='' . | wc
        0       0       0
  $ showgitrepo
  *   102e40a (HEAD -> master_bookmark) Merge branch 'branch1' into master_bookmark
  |\  
  | * 72c4b2c (origin/branch1, branch1) branch1 commit
  * | e1f85f9 (origin/master_bookmark, origin/HEAD) master_bookmark commit
  |/  
  * b6b8fd0 base commit
  *   db1eb8d Merge branch 'branch1' into master_bookmark
  |\  
  | * b73b0f5 branch1 commit
  * | 97232a1 master_bookmark commit
  |/  
  * 97532dd base commit
  *   e6c1afb Resolve merge conflict by taking changes from one of the parents as they are
  |\  
  | * fe275d7 Changed file1 on branch1
  * | a8d85d3 Changed file1 on master_bookmark
  |/  
  *   d657644 Merge branch 'branch1' into master_bookmark
  |\  
  | * 0b37570 Changed file1 on branch1
  * | 9918ff7 Changed file1 on master_bookmark
  |/  
  *   5e502d5 Resolve merge by deleting file1
  |\  
  | * bb56732 Changed file1 on branch1
  * | 9d17996 Changed file1 on master_bookmark
  |/  
  *   a53fce9 Merge branch 'branch1' into master_bookmark
  |\  
  | *   dd218d8 Merge master_bookmark into branch1
  | |\  
  | * | 97d653f Changed file1 on branch1
  * | | 884a03f Changed file2 on master_bookmark
  | |/  
  |/|   
  * | ba3fec7 Changed file1 on master_bookmark
  |/  
  *   8eced2e Merge branch1 into master_bookmark
  |\  
  | * 10b934f Changed file1 on branch1
  * | 8311103 Changed file1 on master_bookmark
  |/  
  * 8ce3eae Add file1
# This should succeed. file1 is different in both parents but one did not touch it since LCA
  $ git_client push origin master_bookmark
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     e1f85f9..102e40a  master_bookmark -> master_bookmark

