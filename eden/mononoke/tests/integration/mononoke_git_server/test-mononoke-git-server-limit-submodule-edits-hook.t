# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

#testcases with-markers-disallowed with-markers-allowed

  $ . "${TEST_FIXTURES}/library.sh"

  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO_SUBMODULE="${TESTTMP}/origin/repo-submodule-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

#if with-markers-allowed
  $ cat >> repos/repo/server.toml <<EOF
  > [[bookmarks]]
  > name="heads/master_bookmark"
  > [[bookmarks.hooks]]
  > hook_name="limit_submodule_edits"
  > [[hooks]]
  > name="limit_submodule_edits"
  > config_json='''{
  > "allow_edits_with_marker": "@update-submodule"
  > }'''
  > EOF
#else
  $ cat >> repos/repo/server.toml <<EOF
  > [[bookmarks]]
  > name="heads/master_bookmark"
  > [[bookmarks.hooks]]
  > hook_name="limit_submodule_edits"
  > [[hooks]]
  > name="limit_submodule_edits"
  > config_json="{}"
  > EOF
#endif

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ old_head=$(git rev-parse HEAD)
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
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

# Create a repo that will be a submodule in the main one
  $ mkdir -p "$GIT_REPO_SUBMODULE"
  $ cd "$GIT_REPO_SUBMODULE"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ cd "$TESTTMP"

# Add some new commits to the cloned repo and push it to remote
  $ cd repo
  $ touch file
  $ git add .
  $ git commit -qam "Commit with a simple file"
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     *..*  master_bookmark -> master_bookmark (glob)

# Add a submodule commit and test pushing it.
  $ git -c protocol.file.allow=always submodule add "$GIT_REPO_SUBMODULE" submodule_path
  Cloning into '$TESTTMP/repo/submodule_path'...
  done.
  $ git add .
  $ git commit -qam "Commit with submodule"

# The git-receive-pack endpoint accepts pushes without moving the bookmarks in the backend
# but stores all the git and bonsai objects in the server

#if with-markers-allowed
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path
    If you did mean to do this, add the following lines to your commit message:
    @update-submodule: submodule_path
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
#else
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
#endif

# Change the commit message and try to push with the marker containing a wrong path.
  $ git commit --amend -qm "@update-submodule: wrong_path"
#if with-markers-allowed
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path
    If you did mean to do this, add the following lines to your commit message:
    @update-submodule: submodule_path
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
#else
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
#endif

# Change the commit message and try to push with the marker containing the path.
  $ git commit --amend -qm "@update-submodule: submodule_path rest of the commit message"
#if with-markers-allowed
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     *..*  master_bookmark -> master_bookmark (glob)
#else
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
#endif

# Add multiple submodules in a commit and test pushing it.
  $ git -c protocol.file.allow=always submodule add "$GIT_REPO_SUBMODULE" submodule_path1
  Cloning into '$TESTTMP/repo/submodule_path1'...
  done.
  $ git -c protocol.file.allow=always submodule add "$GIT_REPO_SUBMODULE" submodule_path2
  Cloning into '$TESTTMP/repo/submodule_path2'...
  done.
  $ git add .
  $ git commit -qam "Commit with multiple submodules"
#if with-markers-allowed
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path1
      - submodule_path2
    If you did mean to do this, add the following lines to your commit message:
    @update-submodule: submodule_path1
    @update-submodule: submodule_path2
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
#else
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path1
      - submodule_path2
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
#endif

# Change the commit message and try to push with the markers containing a wrong path.
  $ git commit --amend -qm "@update-submodule: submodule_path2" -m "@update-submodule: wrong_path rest of the commit message"
#if with-markers-allowed
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path1
    If you did mean to do this, add the following lines to your commit message:
    @update-submodule: submodule_path1
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
#else
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path1
      - submodule_path2
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
#endif

# Change the commit message and try to push with the markers containing correct paths.
  $ git commit --amend -qm "@update-submodule: submodule_path2" -m "@update-submodule: submodule_path1 rest of the commit message"
#if with-markers-allowed
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     *..*  master_bookmark -> master_bookmark (glob)
#else
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path1
      - submodule_path2
    limit_submodule_edits for *: Commit creates or edits submodules at the following paths: (glob)
      - submodule_path
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
#endif
