#require git no-eden no-windows

  $ eagerepo
  $ enable github
  $ export SL_TEST_GH_URL=https://github.com/facebook/test_github_repo.git
  $ . $TESTDIR/git.sh

build up a github repo whose commits correspond to the pull requests of a
native GitHub stack (the commits are already present locally, so no actual
network pull is necessary)

  $ sl init --git repo1
  $ cd repo1
  $ echo a > a1
  $ sl ci -Aqm one
  $ echo a >> a1
  $ sl ci -Aqm two
  $ echo b > b1
  $ sl ci -Aqm three

no commits are linked to pull requests yet
  $ sl log -T '{desc} {github_pull_request_number}\n'
  three 
  two 
  one 

pulling a PR whose body has no stack list footer, but which is part of a
native GitHub stack, links its ancestors using the stacks API
  $ sl pr pull 44 --config extensions.pr_pull=$TESTDIR/github/mock_pull_stacked_pr.py
  imported #44 as f4185fef85f10d46b859c30076243068b0f59245
  $ sl log -T '{desc} {github_pull_request_number}\n'
  three 44
  two 43
  one 42
