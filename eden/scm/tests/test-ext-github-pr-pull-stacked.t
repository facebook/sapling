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

pulling the bottom of a native stack has no ancestors to link: no warning is
printed
  $ sl pr pull 42 --config extensions.pr_pull=$TESTDIR/github/mock_pull_stacked_pr_bottom.py
  imported #42 as ebe5b8faff36687becb7bdbca1e6a61dac428834

pulling a PR that is associated with a native stack but is no longer one of
its open members (e.g., it was merged) warns specifically instead of
pretending there is no stack information; ancestors are not linked
  $ sl pr pull 45 --config extensions.pr_pull=$TESTDIR/github/mock_pull_stacked_pr_merged.py
  imported #45 as f4185fef85f10d46b859c30076243068b0f59245
  #45 is no longer an open member of stack #100.
  Ancestors will not be linked to pull requests.
