
#require eden

  $ setconfig worktree.enabled=true

setup backing repo with linked worktrees

  $ newclientrepo myrepo
  $ touch file.txt
  $ sl add file.txt
  $ sl commit -m "init"
  $ sl worktree add $TESTTMP/linked1
  created linked worktree at $TESTTMP/linked1
  $ sl worktree add $TESTTMP/linked2 --label "feature-x"
  created linked worktree at $TESTTMP/linked2
  $ sl worktree add $TESTTMP/linked_from_subdir
  created linked worktree at $TESTTMP/linked_from_subdir

test worktree remove - missing PATH argument

  $ sl worktree remove
  abort: usage: sl worktree remove PATH [PATH...]
  [255]

test worktree remove - cannot remove main with linked worktrees

  $ sl worktree remove $TESTTMP/myrepo -y
  abort: cannot remove a main worktree with linked worktrees
  [255]

test worktree remove - subdirectory path gives clear error

  $ mkdir -p $TESTTMP/myrepo/subdir
  $ cd $TESTTMP/myrepo/subdir
  $ sl worktree remove . -y
  abort: $TESTTMP/myrepo/subdir is not the root of checkout $TESTTMP/myrepo, not removing
  [255]
  $ cd $TESTTMP/myrepo

test worktree remove - basic remove

  $ sl worktree remove $TESTTMP/linked_from_subdir -y
  removed $TESTTMP/linked_from_subdir
  $ test -d $TESTTMP/linked_from_subdir
  [1]

test worktree remove - list after remove shows fewer entries

  $ sl worktree list
    linked  $TESTTMP/linked1
    linked  $TESTTMP/linked2   feature-x
  * main    $TESTTMP/myrepo

test worktree remove - multiple paths in one command

  $ sl worktree remove $TESTTMP/linked1 $TESTTMP/linked2 -y
  removed $TESTTMP/linked1
  removed $TESTTMP/linked2
  $ test -d $TESTTMP/linked1
  [1]
  $ test -d $TESTTMP/linked2
  [1]
  $ sl worktree list
  this worktree is not part of a group

test worktree remove - duplicate paths are deduplicated

  $ sl worktree add $TESTTMP/dedup1
  created linked worktree at $TESTTMP/dedup1
  $ sl worktree remove $TESTTMP/dedup1 $TESTTMP/dedup1 -y
  removed $TESTTMP/dedup1
  $ test -d $TESTTMP/dedup1
  [1]

re-create linked worktrees for remaining tests

  $ sl worktree add $TESTTMP/linked1
  created linked worktree at $TESTTMP/linked1
  $ sl worktree add $TESTTMP/linked2 --label "feature-x"
  created linked worktree at $TESTTMP/linked2

test worktree remove --all

  $ sl worktree remove --all -y
  removed $TESTTMP/linked1
  removed $TESTTMP/linked2

test worktree remove - group dissolved after all linked removed

  $ sl worktree list
  this worktree is not part of a group

#if no-windows
test worktree remove --all preserves partial progress after eden remove failure

  $ cd $TESTTMP
  $ newclientrepo partial_progress_repo
  $ touch file.txt
  $ sl add file.txt
  $ sl commit -m "init"
  $ sl worktree add $TESTTMP/remove_all_fail
  created linked worktree at $TESTTMP/remove_all_fail
  $ sl worktree add $TESTTMP/remove_all_ok
  created linked worktree at $TESTTMP/remove_all_ok
  $ original_eden_command="$(sl config edenfs.command)"
  $ cat > $TESTTMP/failing_eden <<EOF
  > #!/bin/sh
  > set -eu
  > original='$original_eden_command'
  > is_remove=0
  > last=
  > for arg in "\$@"; do
  >   if [ "\$arg" = "remove" ]; then
  >     is_remove=1
  >   fi
  >   last="\$arg"
  > done
  > if [ "\$is_remove" = "1" ] && [ "\$last" = "$TESTTMP/remove_all_fail" ]; then
  >   echo "injected eden remove failure for \$last" >&2
  >   exit 1
  > fi
  > exec "\$original" "\$@"
  > EOF
  $ chmod +x $TESTTMP/failing_eden
  $ setconfig edenfs.command=$TESTTMP/failing_eden
  $ sl worktree remove --all -y
  failed to remove $TESTTMP/remove_all_fail: eden remove failed for $TESTTMP/remove_all_fail: injected eden remove failure for $TESTTMP/remove_all_fail
  removed $TESTTMP/remove_all_ok
  abort: eden remove failed for $TESTTMP/remove_all_fail: injected eden remove failure for $TESTTMP/remove_all_fail
  [255]
  $ test -d $TESTTMP/remove_all_fail
  $ test -d $TESTTMP/remove_all_ok
  [1]
  $ sl worktree list
  * main    $TESTTMP/partial_progress_repo
    linked  $TESTTMP/remove_all_fail
#endif

test worktree remove - pre-worktree-remove hook fires with correct env vars

  $ cd $TESTTMP
  $ newclientrepo pre_hook_remove_repo
  $ touch file.txt
  $ sl add file.txt
  $ sl commit -m "init"
  $ sl worktree add $TESTTMP/pre_hook_rm1
  created linked worktree at $TESTTMP/pre_hook_rm1
#if windows
  $ setconfig hooks.pre-worktree-remove="echo PATH:%HG_PATH%"
  $ sl worktree remove $TESTTMP/pre_hook_rm1 -y
  PATH:$TESTTMP?pre_hook_rm1\r (esc) (glob)
  removed $TESTTMP/pre_hook_rm1
#else
  $ setconfig hooks.pre-worktree-remove="echo PATH:\$HG_PATH"
  $ sl worktree remove $TESTTMP/pre_hook_rm1 -y
  PATH:$TESTTMP/pre_hook_rm1
  removed $TESTTMP/pre_hook_rm1
#endif

test worktree remove - pre-worktree-remove hook failure is best effort for single remove

  $ sl worktree add $TESTTMP/pre_hook_blocked
  created linked worktree at $TESTTMP/pre_hook_blocked
#if windows
  $ setconfig "hooks.pre-worktree-remove=cmd /c exit 1"
#else
  $ setconfig hooks.pre-worktree-remove=false
#endif
  $ sl worktree remove $TESTTMP/pre_hook_blocked -y
  removed $TESTTMP/pre_hook_blocked
  $ test -d $TESTTMP/pre_hook_blocked
  [1]

test worktree remove - pre-worktree-remove hook failure in --all mode still removes worktree

  $ sl worktree add $TESTTMP/pre_hook_all1
  created linked worktree at $TESTTMP/pre_hook_all1
#if windows
  $ setconfig "hooks.pre-worktree-remove=cmd /c exit 1"
#else
  $ setconfig hooks.pre-worktree-remove=false
#endif
  $ sl worktree remove --all -y
  removed $TESTTMP/pre_hook_all1
  $ test -d $TESTTMP/pre_hook_all1
  [1]
  $ sl worktree list
  this worktree is not part of a group

test worktree remove - pre-worktree-remove hook does not fire when user declines confirmation

  $ sl worktree add $TESTTMP/pre_hook_decline
  created linked worktree at $TESTTMP/pre_hook_decline
  $ setconfig hooks.pre-worktree-remove="echo HOOK_FIRED"
  $ echo n | sl worktree remove $TESTTMP/pre_hook_decline
  abort: running non-interactively, use -y instead
  [255]
  $ test -d $TESTTMP/pre_hook_decline
