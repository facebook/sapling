#require git no-eden no-windows

  $ eagerepo
  $ enable github
  $ export SL_TEST_GH_URL=https://github.com/facebook/test_github_repo.git
  $ . $TESTDIR/git.sh
  $ configure github.pr-workflow=stacked

build up a github repo

  $ sl init --git repo1
  $ cd repo1
  $ echo a > a1
  $ sl ci -Aqm one
  $ echo a >> a1
  $ sl ci -Aqm two

confirm it is a 'github_repo'
  $ sl log -r. -T '{github_repo}\n'
  True

submitting a stack of 2 commits creates the PRs (with chained bases, like the
"single" workflow, and without the stack list footer) and links them into a
native GitHub stack
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_create_stacked_prs.py
  pushing 2 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/42
  created new pull request: https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42
  created stack: https://github.com/facebook/test_github_repo/stacks/100

adding a commit on top and resubmitting appends the new PR to the existing
stack; the bases of #42 and #43 are NOT updated via the API (GitHub rejects
base changes for PRs in a native stack)
  $ echo b > b1
  $ sl ci -Aqm three
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_extend_stacked_prs.py
  #42 is up-to-date
  #43 is up-to-date
  pushing 1 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/44
  updated body for https://github.com/facebook/test_github_repo/pull/44
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42
  added #44 to stack #100

if the stack on GitHub has diverged from the local stack, warn without
modifying it
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_diverged_stack.py
  #42 is up-to-date
  #43 is up-to-date
  #44 is up-to-date
  no pull requests to update
  warning: stack #100 on GitHub (#43, #42, #44) does not match your local stack (#42, #43, #44); not updating it
  hint[pr-submit-restack]: use 'sl pr submit --restack' to dissolve the stack on GitHub and recreate it to match your local stack

with --restack, the diverged stack is dissolved and recreated to match the
local stack
  $ sl pr submit --restack --config extensions.pr_submit=$TESTDIR/github/mock_diverged_stack.py
  #42 is up-to-date
  #43 is up-to-date
  #44 is up-to-date
  no pull requests to update
  recreated stack: https://github.com/facebook/test_github_repo/stacks/101

if the stack on GitHub has diverged AND there are local changes to push,
abort before updating any bases or pushing anything
  $ echo b >> b1
  $ sl amend
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_diverged_stack_dirty.py
  #42 is up-to-date
  #43 is up-to-date
  stack #100 on GitHub (#43, #42, #44) does not match your local stack (#42, #43, #44); not updating it
  hint[pr-submit-restack]: use 'sl pr submit --restack' to dissolve the stack on GitHub and recreate it to match your local stack
  abort: stack on GitHub has diverged from your local stack
  [255]

with --restack, the diverged stack is dissolved up front so that base
branches can be updated, then recreated from the local stack
  $ sl pr submit --restack --config extensions.pr_submit=$TESTDIR/github/mock_diverged_stack_dirty.py
  #42 is up-to-date
  #43 is up-to-date
  updated base for https://github.com/facebook/test_github_repo/pull/44
  updated base for https://github.com/facebook/test_github_repo/pull/43
  updated base for https://github.com/facebook/test_github_repo/pull/42
  pushing 1 to https://github.com/facebook/test_github_repo.git
  updated body for https://github.com/facebook/test_github_repo/pull/44
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/42
  created stack: https://github.com/facebook/test_github_repo/stacks/101
