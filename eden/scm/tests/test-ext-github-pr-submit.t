#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible

  $ enable github
  $ . $TESTDIR/git.sh

build up a github repo

  $ sl init --git repo1
  $ cd repo1
  $ setconfig paths.default=https://github.com/facebook/test_github_repo.git
  $ echo a > a1
  $ sl ci -Am addfile
  adding a1

confirm it is a 'github_repo'
  $ sl log -r. -T '{github_repo}\n'
  True

test sending pr
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_create_one_pr.py
  pushing 1 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/1
  updated body for https://github.com/facebook/test_github_repo/pull/1

test sending pr with a 'default-push' path
  $ setconfig paths.default-push=https://github.com/contributor/fork_github_repo.git
  $ sl pr unlink -r .
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_create_one_pr_from_contributor_repo.py
  pushing 1 to https://github.com/contributor/fork_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/2
  updated body for https://github.com/facebook/test_github_repo/pull/2
