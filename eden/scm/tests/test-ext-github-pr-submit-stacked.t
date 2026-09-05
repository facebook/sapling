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

inserting a new commit into the middle of the stack diverges from the stack
on GitHub even though the existing PRs are in the same order (a stack can
only grow at the top): without --restack, abort before updating or pushing
anything
  $ sl goto -q 'desc(one)'
  $ echo c > c1
  $ sl ci -Aqm insert
  $ sl rebase -qs 'desc(two)' -d .
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_insert_mid_stack.py
  #42 is up-to-date
  new pull requests would be inserted below the top of stack #100 on GitHub (#42, #43, #44), which requires recreating the stack; not updating it
  hint[pr-submit-restack]: use 'sl pr submit --restack' to dissolve the stack on GitHub and recreate it to match your local stack
  abort: stack on GitHub has diverged from your local stack
  [255]

with --restack, the stack is dissolved up front, the new PR is created with
its base chained into the stack, and the stack is recreated in the new order
  $ sl pr submit --restack --config extensions.pr_submit=$TESTDIR/github/mock_insert_mid_stack_restack.py
  #42 is up-to-date
  updated base for https://github.com/facebook/test_github_repo/pull/44
  updated base for https://github.com/facebook/test_github_repo/pull/43
  updated base for https://github.com/facebook/test_github_repo/pull/42
  pushing 3 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/facebook/test_github_repo/pull/45
  updated body for https://github.com/facebook/test_github_repo/pull/44
  updated body for https://github.com/facebook/test_github_repo/pull/43
  updated body for https://github.com/facebook/test_github_repo/pull/45
  updated body for https://github.com/facebook/test_github_repo/pull/42
  created stack: https://github.com/facebook/test_github_repo/stacks/101

if the stacks API query fails while there are local changes to push, fail
closed: without knowing the state of the stack, pushing could corrupt it
  $ sl goto -q 'desc(three)'
  $ echo b >> b1
  $ sl amend
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_stack_query_failure.py
  #42 is up-to-date
  #45 is up-to-date
  #43 is up-to-date
  warning, could not query stacks for #42: mock stacks API failure
  abort: could not determine the state of the stack on GitHub; re-run 'pr submit' to retry
  [255]

if dissolving a diverged stack only partially succeeds (merged or queued
PRs cannot be unstacked), abort rather than proceed against the remnant
  $ sl pr submit --restack --config extensions.pr_submit=$TESTDIR/github/mock_unstack_remnant.py
  #42 is up-to-date
  #45 is up-to-date
  #43 is up-to-date
  abort: stack #100 was only partially dissolved: #43 could not be unstacked (merged or queued pull requests are left in place)
  [255]

a single open pull request whose stack on GitHub has more members is a
divergence; --restack refuses to dissolve the stack since it could not be
recreated with fewer than two pull requests
  $ cd ..
  $ sl init --git repo2
  $ cd repo2
  $ echo a > a1
  $ sl ci -Aqm "one
  > 
  > Pull Request resolved: https://github.com/facebook/test_github_repo/pull/42"
  $ echo a >> a1
  $ sl amend
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_restack_too_small.py
  stack #100 on GitHub (#42, #43) does not match your local stack (#42); not updating it
  hint[pr-submit-restack]: use 'sl pr submit --restack' to dissolve the stack on GitHub and recreate it to match your local stack
  abort: stack on GitHub has diverged from your local stack
  [255]
  $ sl pr submit --restack --config extensions.pr_submit=$TESTDIR/github/mock_restack_too_small.py
  abort: --restack would leave stack #100 with fewer than two pull requests; dissolve it on GitHub instead if that is intended
  [255]

a closed stack is treated the same as no stack: the base branch is updated
via the API and no stack operations are attempted
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_closed_stack.py
  updated base for https://github.com/facebook/test_github_repo/pull/42
  pushing 1 to https://github.com/facebook/test_github_repo.git
  updated body for https://github.com/facebook/test_github_repo/pull/42

if dissolving the stack during the post-submit sync partially fails, warn
without recreating the stack (the pull requests themselves were already
submitted)
  $ echo b > b1
  $ sl ci -Aqm "two
  > 
  > Pull Request resolved: https://github.com/facebook/test_github_repo/pull/43"
  $ sl pr submit --restack --config extensions.pr_submit=$TESTDIR/github/mock_unstack_remnant_sync.py
  #42 is up-to-date
  #43 is up-to-date
  no pull requests to update
  warning, stack #100 was only partially dissolved: #43 could not be unstacked (merged or queued pull requests are left in place)

stacks are not supported across forks: pull requests are created against the
upstream default branch and a warning is printed instead of creating a stack
  $ cd ..
  $ sl init --git repo3
  $ cd repo3
  $ echo a > a1
  $ sl ci -Aqm one
  $ echo a >> a1
  $ sl ci -Aqm two
  $ sl pr submit --config extensions.pr_submit=$TESTDIR/github/mock_fork_stacked.py
  pushing 2 to https://github.com/facebook/test_github_repo.git
  created new pull request: https://github.com/upstream/test_github_repo/pull/42
  created new pull request: https://github.com/upstream/test_github_repo/pull/43
  updated body for https://github.com/upstream/test_github_repo/pull/43
  updated body for https://github.com/upstream/test_github_repo/pull/42
  warning: GitHub does not support stacks across forks; pull requests were submitted without a stack
