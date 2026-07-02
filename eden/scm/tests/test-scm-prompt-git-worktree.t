#debugruntest-incompatible

#require bash no-eden git

  $ eagerepo
Initialize scm prompt with worktree-name display enabled
  $ . $TESTDIR/../contrib/scm-prompt.sh
  $ export SCM_PROMPT_SHOW_WORKTREE=1

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

Add a linked worktree on a new branch and check the prompt shows branch | worktree-name
  $ git worktree add -q ../wt1 -b feature
  $ cd ../wt1
  $ _scm_prompt "%s\n"
  feature|wt1

Relative gitdir pointers should resolve from subdirectories
  $ cd ../main
  $ git worktree add -q ../wt-rel -b relative-feature
  $ cd ../wt-rel
  $ echo "gitdir: ../main/.git/worktrees/wt-rel" > .git
  $ cat .git
  gitdir: ../main/.git/worktrees/wt-rel
  $ _scm_prompt "%s\n"
  relative-feature|wt-rel
  $ mkdir sub
  $ cd sub
  $ _scm_prompt "%s\n"
  relative-feature|wt-rel
  $ cd ../../wt1

A subdirectory of the worktree should walk up and still resolve correctly
  $ mkdir sub
  $ cd sub
  $ _scm_prompt "%s\n"
  feature|wt1
  $ cd ..

Per-worktree state in a linked worktree (rebase) should appear in the prompt
  $ cd ../main
  $ echo b2 > a
  $ cmd git commit -qam "bb2"
  (master)
  $ cd ../wt1
  $ echo c > a
  $ cmd git commit -qam "cc"
  (feature|wt1)
  $ quietcmd git rebase --merge master
  (*|wt1|REBASE-*|feature) (glob)
  $ cmd git rebase --abort
  (feature|wt1)

A detached worktree should show the short hash plus the worktree name
  $ cd ../main
  $ git worktree add -q --detach ../wt2 HEAD~1
  $ cd ../wt2
  $ _scm_prompt "(%s)\n"
  (d94a2a17|wt2)

The main checkout has no worktree-name suffix
  $ cd ../main
  $ _scm_prompt "(%s)\n"
  (master)

Malformed .git pointer (missing gitdir: line) should produce empty output, not garbage
  $ cd ..
  $ mkdir bad-wt
  $ cd bad-wt
  $ echo "junk content" > .git
  $ _scm_prompt "(%s)\n"

Without SCM_PROMPT_SHOW_WORKTREE the worktree name is not appended
  $ cd ../wt1
  $ unset SCM_PROMPT_SHOW_WORKTREE
  $ _scm_prompt "(%s)\n"
  (feature)
  $ export SCM_PROMPT_SHOW_WORKTREE=1
  $ _scm_prompt "(%s)\n"
  (feature|wt1)

A submodule .git pointer (gitdir under .git/modules/) must NOT get a worktree suffix
  $ cd ..
  $ mkdir -p main-with-sub/.git/modules/sub
  $ echo "ref: refs/heads/main-branch" > main-with-sub/.git/modules/sub/HEAD
  $ mkdir main-with-sub/sub
  $ cd main-with-sub/sub
  $ echo "gitdir: ../.git/modules/sub" > .git
  $ _scm_prompt "(%s)\n"
  (main-branch)
