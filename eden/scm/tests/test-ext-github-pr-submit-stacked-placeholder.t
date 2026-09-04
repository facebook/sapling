#require git no-eden no-windows

  $ eagerepo
  $ enable github
  $ export SL_TEST_GH_URL=https://github.com/facebook/test_github_repo.git
  $ . $TESTDIR/git.sh
  $ configure github.pr-workflow=stacked

build up a github repo

  $ sl init --git repo1
  $ cd repo1
  $ setconfig github.placeholder-strategy=True
  $ echo a > a1
  $ sl ci -Aqm one
  $ echo a >> a1
  $ sl ci -Aqm two

submitting a stack of 2 commits with the placeholder strategy creates the PRs
(with chained bases) and links them into a native GitHub stack

  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_create_stacked_prs_placeholder.py
  pushing 2 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/42
  created new pull request: https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42
  created stack: https://github.com/facebook/test_github_repo/stacks/100
