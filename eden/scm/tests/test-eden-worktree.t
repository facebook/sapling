
#require eden

  $ setconfig worktree.enabled=true

# ============================================================
# End-to-end lifecycle
# ============================================================

test full lifecycle

  $ newclientrepo e2e_repo
  $ touch file.txt && hg add file.txt && hg commit -m "init"

  $ hg worktree add $TESTTMP/e2e_linked1 --label "dev"
  created linked worktree at $TESTTMP/e2e_linked1
  $ hg worktree add $TESTTMP/e2e_linked2 --label "staging"
  created linked worktree at $TESTTMP/e2e_linked2

  $ hg worktree list
    linked  $TESTTMP/e2e_linked1   dev
    linked  $TESTTMP/e2e_linked2   staging
  * main    $TESTTMP/e2e_repo

  $ hg worktree label $TESTTMP/e2e_linked1 "dev-v2"
  label set for $TESTTMP/e2e_linked1

  $ hg worktree list
    linked  $TESTTMP/e2e_linked1   dev-v2
    linked  $TESTTMP/e2e_linked2   staging
  * main    $TESTTMP/e2e_repo

  $ hg worktree remove $TESTTMP/e2e_linked1 -y
  removed $TESTTMP/e2e_linked1

  $ hg worktree list
    linked  $TESTTMP/e2e_linked2   staging
  * main    $TESTTMP/e2e_repo

  $ hg worktree label $TESTTMP/e2e_linked2 --remove
  label removed for $TESTTMP/e2e_linked2

  $ hg worktree list
    linked  $TESTTMP/e2e_linked2
  * main    $TESTTMP/e2e_repo

  $ hg worktree remove --all -y
  removed $TESTTMP/e2e_linked2

  $ hg worktree list
  this worktree is not part of a group

# ============================================================
# Auto-cleanup
# ============================================================

test auto-cleanup: one missing linked worktree

  $ hg worktree add $TESTTMP/cleanup_wt1
  created linked worktree at $TESTTMP/cleanup_wt1
  $ hg worktree add $TESTTMP/cleanup_wt2
  created linked worktree at $TESTTMP/cleanup_wt2

  $ hg worktree list
    linked  $TESTTMP/cleanup_wt1
    linked  $TESTTMP/cleanup_wt2
  * main    $TESTTMP/e2e_repo

externally remove one linked worktree (eden remove, not sl worktree remove)

  $ EDENFSCTL_ONLY_RUST=true eden rm -y $TESTTMP/cleanup_wt1 > /dev/null 2>&1

  $ hg worktree list
    linked  $TESTTMP/cleanup_wt2
  * main    $TESTTMP/e2e_repo

test auto-cleanup: all linked worktrees missing - group dissolved

  $ EDENFSCTL_ONLY_RUST=true eden rm -y $TESTTMP/cleanup_wt2 > /dev/null 2>&1

  $ hg worktree list
  this worktree is not part of a group

  $ hg worktree list
  this worktree is not part of a group
