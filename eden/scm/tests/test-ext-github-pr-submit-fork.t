#require git no-eden no-windows

  $ eagerepo
  $ enable github
  $ export SL_TEST_GH_URL=https://github.com/aionic-labs/sapling.git
  $ . $TESTDIR/git.sh
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > version-age-threshold-days=0
  > EOF
  $ configure github.pr-workflow=overlap

Create a commit with an existing association to the fork's upstream repository.

  $ sl init --git repo1
  $ cd repo1
  $ echo a > a1
  $ sl ci -Aqm one
  $ sl pr link https://github.com/facebook/sapling/pull/1432 -r .

Submitting directly to the configured remote ignores the upstream association.

  $ sl --config github.submit-to-upstream=false pr submit --rev . --draft --reviewer alice --config extensions.pr_submit=$TESTDIR/github/mock_create_pr_in_fork.py
  pushing 1 to https://github.com/aionic-labs/sapling.git
  created new pull request: https://github.com/aionic-labs/sapling/pull/42
  updated body for https://github.com/aionic-labs/sapling/pull/42
  requested reviewers for https://github.com/aionic-labs/sapling/pull/42
