#require git no-eden no-windows

Test that `sl pr submit` can post GitHub compare links for updated PRs.

  $ eagerepo
  $ enable github
  $ setconfig hint.ack='*'
  $ export SL_TEST_GH_URL=https://github.com/facebook/test_github_repo.git
  $ . $TESTDIR/git.sh

Overlap workflow posts a delta comment for the updated PR.

  $ setconfig github.pr-workflow=overlap
  $ sl init --git overlap
  $ cd overlap
  $ echo a > a1
  $ sl ci -Aqm one
  $ echo a >> a1
  $ sl ci -Aqm two
  $ sl pr submit --config extensions.pr_submit_create_overlap=$TESTDIR/github/mock_create_prs.py
  pushing 2 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/42
  created new pull request: https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42

  $ export SL_TEST_PR42_HEAD=$(sl log -r ".^" -T "{node}")
  $ export SL_TEST_PR43_HEAD=$(sl log -r "." -T "{node}")
  $ echo b >> a1
  $ sl amend -qm "two amended"
  $ export SL_TEST_DELTA_PR=43
  $ export SL_TEST_DELTA_OLD=$SL_TEST_PR43_HEAD
  $ export SL_TEST_DELTA_NEW=$(sl log -r "." -T "{node}")
  $ sl pr submit --config github.pr-submit-comment-deltas=true --config extensions.pr_submit_delta_overlap=$TESTDIR/github/mock_delta_update.py
  #42 is up-to-date
  pushing 1 to https://github.com/facebook/test_github_repo.git
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42
  posted update delta for https://github.com/facebook/test_github_repo/pull/43

Single workflow posts the same style of delta comment.

  $ cd ..
  $ setconfig github.pr-workflow=single
  $ sl init --git single
  $ cd single
  $ echo a > a1
  $ sl ci -Aqm one
  $ echo a >> a1
  $ sl ci -Aqm two
  $ sl pr submit --config extensions.pr_submit_create_single=$TESTDIR/github/mock_create_prs.py
  pushing 2 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/42
  created new pull request: https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42

  $ export SL_TEST_PR42_HEAD=$(sl log -r ".^" -T "{node}")
  $ export SL_TEST_PR43_HEAD=$(sl log -r "." -T "{node}")
  $ echo b >> a1
  $ sl amend -qm "two amended"
  $ export SL_TEST_DELTA_PR=43
  $ export SL_TEST_DELTA_OLD=$SL_TEST_PR43_HEAD
  $ export SL_TEST_DELTA_NEW=$(sl log -r "." -T "{node}")
  $ sl pr submit --config github.pr-submit-comment-deltas=true --config extensions.pr_submit_delta_single=$TESTDIR/github/mock_delta_update.py
  #42 is up-to-date
  updated base for https://github.com/facebook/test_github_repo/pull/43
  updated base for https://github.com/facebook/test_github_repo/pull/42
  pushing 1 to https://github.com/facebook/test_github_repo.git
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42
  posted update delta for https://github.com/facebook/test_github_repo/pull/43
