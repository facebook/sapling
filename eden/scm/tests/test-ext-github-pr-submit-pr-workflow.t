#require git no-eden no-windows

#inprocess-hg-incompatible

  $ eagerepo
  $ enable github
  $ export SL_TEST_GH_URL=https://github.com/facebook/test_github_repo.git
  $ . $TESTDIR/git.sh

CLI flag should override config and use single workflow behavior.

  $ configure github.pr-workflow=overlap
  $ sl init --git repo-single
  $ cd repo-single
  $ echo a > a1
  $ sl ci -Aqm one
  $ echo a >> a1
  $ sl ci -Aqm two
  $ sl pr submit --pr-workflow single --config extensions.pr_submit=$TESTDIR/github/mock_create_prs_single_workflow.py
  pushing 2 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/42
  created new pull request: https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42

CLI flag should override config and use overlap workflow behavior.

  $ cd ..
  $ configure github.pr-workflow=single
  $ sl init --git repo-overlap
  $ cd repo-overlap
  $ echo a > a1
  $ sl ci -Aqm one
  $ echo a >> a1
  $ sl ci -Aqm two
  $ sl pr submit --pr-workflow overlap --config extensions.pr_submit=$TESTDIR/github/mock_create_prs_overlap_workflow.py
  pushing 2 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/42
  created new pull request: https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42

Invalid workflow values should warn and default to overlap.

  $ cd ..
  $ sl init --git repo-banana
  $ cd repo-banana
  $ echo a > a1
  $ sl ci -Aqm one
  $ echo a >> a1
  $ sl ci -Aqm two
  $ sl pr submit --pr-workflow banana --config extensions.pr_submit=$TESTDIR/github/mock_create_prs_overlap_workflow.py
  unrecognized config for github.pr_workflow: defaulting to 'overlap'pushing 2 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/42
  created new pull request: https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42
