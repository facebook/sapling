#debugruntest-incompatible

#chg-compatible
#require bash no-eden git

  $ eagerepo
Initialize scm prompt
  $ . $TESTDIR/../contrib/scm-prompt.sh

  $ cmd() {
  >   "$@"
  >   _scm_prompt "(%s)\n"
  > }
  $ git() {
  >   command git -c init.defaultBranch=master "$@"
  > }

  $ quietcmd() {
  >   "$@" &> /dev/null
  >   _scm_prompt "(%s)\n"
  > }

Set env to make commands repeatable
  $ GIT_AUTHOR_NAME="Test User"
  $ export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL="test@user.org"
  $ export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2006-07-03 17:18:43 +0200"
  $ export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="Test User"
  $ export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="test@user.org"
  $ export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="2006-07-03 17:18:43 +0200"
  $ export GIT_COMMITTER_DATE

Set up main repo with two commits so we have a parent to detach to
  $ git init -q main
  $ cd main
  $ echo a > a
  $ git add a
  $ cmd git commit -qm "aa"
  (master)
  $ echo b > a
  $ cmd git commit -qam "bb"
  (master)

Add a linked worktree on a new branch and check the prompt resolves the gitdir pointer
  $ git worktree add -q ../wt1 -b feature
  $ cd ../wt1
  $ _scm_prompt "%s\n"
  feature

Relative gitdir pointers should resolve from subdirectories
  $ cd ../main
  $ git worktree add -q ../wt-rel -b relative-feature
  $ cd ../wt-rel
  $ echo "gitdir: ../main/.git/worktrees/wt-rel" > .git
  $ cat .git
  gitdir: ../main/.git/worktrees/wt-rel
  $ _scm_prompt "%s\n"
  relative-feature
  $ mkdir sub
  $ cd sub
  $ _scm_prompt "%s\n"
  relative-feature
  $ cd ../../wt1

A subdirectory of the worktree should walk up and still resolve correctly
  $ mkdir sub
  $ cd sub
  $ _scm_prompt "%s\n"
  feature
  $ cd ..

Per-worktree state in a linked worktree (rebase) should appear in the prompt
  $ cd ../main
  $ echo b2 > a
  $ cmd git commit -qam "bb2"
  (master)
  $ cd ../wt1
  $ echo c > a
  $ cmd git commit -qam "cc"
  (feature)
  $ quietcmd git rebase --merge master
  (*|REBASE-*|feature) (glob)
  $ cmd git rebase --abort
  (feature)

A detached worktree should show the short hash, not a branch name
  $ cd ../main
  $ git worktree add -q --detach ../wt2 HEAD~1
  $ cd ../wt2
  $ _scm_prompt "(%s)\n"
  (d94a2a17)

Malformed .git pointer (missing gitdir: line) should produce empty output, not garbage
  $ cd ..
  $ mkdir bad-wt
  $ cd bad-wt
  $ echo "junk content" > .git
  $ _scm_prompt "(%s)\n"
