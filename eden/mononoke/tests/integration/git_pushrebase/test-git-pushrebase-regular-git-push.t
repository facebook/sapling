# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test that git-pushrebase works properly when used as a simple wrapper to git
# push, i.e. it just logs metadata and runs `git push $@`.

-- Set dates,names and emails for deterministic commits
  $ date="01/01/0000 00:00 +0000"
  $ name="mononoke"
  $ email="mononoke@mononoke"
  $ export GIT_COMMITTER_DATE="$date"
  $ export GIT_COMMITTER_NAME="$name"
  $ export GIT_COMMITTER_EMAIL="$email"
  $ export GIT_AUTHOR_DATE="$date"
  $ export GIT_AUTHOR_NAME="$name"
  $ export GIT_AUTHOR_EMAIL="$email"

  $ GIT_REPO=$TESTTMP/git_repo
  $ GIT_SERVER_REPO=$TESTTMP/git_server_repo

  $ TEST_BIN=$TESTTMP/bin
  $ mkdir "$TEST_BIN"
  $ ln -s $GIT_PUSHREBASE $TEST_BIN/git-pushrebase
  $ export PATH=$TEST_BIN:$PATH
  $ which git-pushrebase
  $TESTTMP/bin/git-pushrebase
  $ which git-pushrebase
  $TESTTMP/bin/git-pushrebase


-- Create local git repo to push to
  $ mkdir $GIT_SERVER_REPO
  $ cd $GIT_SERVER_REPO
  $ git init -q --bare
  $ git log --oneline
  fatal: your current branch 'master' does not have any commits yet
  [128]

-- Create a git repo and commit something to it
  $ mkdir $GIT_REPO
  $ cd $GIT_REPO
  $ git init -q
  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)
  $ echo "file" > foo
  $ git add .
  $ git commit -q -m "First commit" 


  $ git remote add origin "file://$TESTTMP/git_server_repo"
  $ git config --global push.default current

-- Run git pushrebase, which will just run git push
  $ git pushrebase "file://${GIT_SERVER_REPO}" master
  To file://$TESTTMP/git_server_repo
   * [new branch]      master -> master
  
  

-- Test different combinations of arguments

-- Should fail because of missing origin and refspec
  $ git pushrebase
  Everything up-to-date
  
  

-- Should fail because of missing refspec
  $ git pushrebase "file://${GIT_SERVER_REPO}"
  Everything up-to-date
  
  
-- Should fail because of missing repository
  $ git pushrebase master
  fatal: 'master' does not appear to be a git repository
  fatal: Could not read from remote repository.
  
  Please make sure you have the correct access rights
  and the repository exists.
  
  

-- Check git server repo
  $ cd $GIT_SERVER_REPO
  $ git log --oneline
  8eb7a82 First commit
