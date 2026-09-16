#require git no-eden no-windows

  $ eagerepo
  $ enable github
  $ export SL_TEST_GH_URL=https://github.com/facebook/test_github_repo.git
  $ . $TESTDIR/git.sh
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > version-age-threshold-days=0
  > EOF
  $ configure github.pr-workflow=overlap

build a two-commit stack

  $ sl init --git repo1
  $ cd repo1
  $ echo a > a1
  $ sl ci -Aqm one
  $ echo a >> a1
  $ sl ci -Aqm two

submit only the selected revision as a draft and request reviewers

  $ sl pr submit --rev . --draft --reviewer alice --reviewer @bob --config extensions.pr_submit=$TESTDIR/github/mock_create_selected_pr.py
  pushing 1 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/42
  updated body for https://github.com/facebook/test_github_repo/pull/42
  requested reviewers for https://github.com/facebook/test_github_repo/pull/42
