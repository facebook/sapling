
#require eden

sections that inject edenfsctl failures switch to the subprocess removal path
explicitly; everything else exercises the default in-process path

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

test worktree remove - basic remove cleans redirection backing storage

#if no-windows
  $ eden redirect add --mount $TESTTMP/linked_from_subdir scratch symlink
  $ backing="$(readlink $TESTTMP/linked_from_subdir/scratch)"
  $ echo contents > "$backing/contents"
#endif
#if linux
  $ eden redirect add --mount $TESTTMP/linked_from_subdir bind1 bind
  $ eden redirect list --json --mount $TESTTMP/linked_from_subdir > $TESTTMP/redirections.json
  $ bind_backing1="$(python -c 'import json, sys; print(next(r["target"] for r in json.load(open(sys.argv[-1])) if r["repo_path"] == "bind1"))' $TESTTMP/redirections.json)"
  $ echo contents > $TESTTMP/linked_from_subdir/bind1/contents
  $ test -f "$bind_backing1/contents"
#endif
  $ sl worktree remove $TESTTMP/linked_from_subdir -y
  Unmounting redirections... (no-windows !)
  Deleting redirection backing storage: *bind1... (glob) (linux !)
  Deleting redirection backing storage: *scratch... (glob) (no-windows !)
  Unmounting checkout...
  Deleting EdenFS checkout state...
  removed $TESTTMP/linked_from_subdir
  $ test -d $TESTTMP/linked_from_subdir
  [1]
#if no-windows
  $ test -e "$backing"
  [1]
  $ python -c 'import os, sys; print(os.path.exists(os.path.dirname(sys.argv[-1])))' "$backing"
  False
#endif
#if linux
  $ test -e "$bind_backing1"
  [1]
#endif

test worktree remove - list after remove shows fewer entries

  $ sl worktree list
    linked  $TESTTMP/linked1
    linked  $TESTTMP/linked2   feature-x
  * main    $TESTTMP/myrepo

test worktree remove - multiple paths in one command

  $ sl worktree remove $TESTTMP/linked1 $TESTTMP/linked2 -y
  Unmounting checkout...
  Deleting EdenFS checkout state...
  removed $TESTTMP/linked1
  Unmounting checkout...
  Deleting EdenFS checkout state...
  removed $TESTTMP/linked2
  $ test -d $TESTTMP/linked1
  [1]
  $ test -d $TESTTMP/linked2
  [1]
  $ sl worktree list
  this worktree is not part of a group

#if no-windows
test worktree remove - multiple paths preserves partial progress after eden remove failure

  $ cd $TESTTMP
  $ newclientrepo multipath_partial_repo
  $ touch file.txt
  $ sl add file.txt
  $ sl commit -m "init"
  $ sl worktree add $TESTTMP/multipath_ok
  created linked worktree at $TESTTMP/multipath_ok
  $ sl worktree add $TESTTMP/multipath_z_fail
  created linked worktree at $TESTTMP/multipath_z_fail
  $ original_eden_command="$(sl config edenfs.command)"
  $ cat > $TESTTMP/failing_eden_multipath <<EOF
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
  > if [ "\$is_remove" = "1" ] && [ "\$last" = "$TESTTMP/multipath_z_fail" ]; then
  >   echo "injected eden remove failure for \$last" >&2
  >   exit 1
  > fi
  > exec "\$original" "\$@"
  > EOF
  $ chmod +x $TESTTMP/failing_eden_multipath
  $ setconfig edenfs.command=$TESTTMP/failing_eden_multipath
  $ setconfig worktree.direct-eden-removal=false
  $ sl worktree remove $TESTTMP/multipath_ok $TESTTMP/multipath_z_fail -y
  removed $TESTTMP/multipath_ok
  failed to remove $TESTTMP/multipath_z_fail: eden remove failed for $TESTTMP/multipath_z_fail: injected eden remove failure for $TESTTMP/multipath_z_fail
  abort: eden remove failed for $TESTTMP/multipath_z_fail: injected eden remove failure for $TESTTMP/multipath_z_fail
  [255]
  $ test -d $TESTTMP/multipath_ok
  [1]
  $ test -d $TESTTMP/multipath_z_fail
  $ sl worktree list
  * main    $TESTTMP/multipath_partial_repo
    linked  $TESTTMP/multipath_z_fail
  $ cd $TESTTMP/myrepo
#endif

test worktree remove - duplicate paths are deduplicated

  $ sl worktree add $TESTTMP/dedup1
  created linked worktree at $TESTTMP/dedup1
  $ sl worktree remove $TESTTMP/dedup1 $TESTTMP/dedup1 -y
  Unmounting checkout...
  Deleting EdenFS checkout state...
  removed $TESTTMP/dedup1
  $ test -d $TESTTMP/dedup1
  [1]

#if no-windows
test worktree remove - direct removal reports skipped backing cleanup

  $ sl worktree add $TESTTMP/direct_missing_config
  created linked worktree at $TESTTMP/direct_missing_config
  $ rm $TESTTMP/direct_missing_config/.eden/client/config.toml
  $ sl worktree remove $TESTTMP/direct_missing_config -y
  Skipping redirection backing cleanup for $TESTTMP/direct_missing_config because its checkout config is missing. APFS volumes left behind require manual cleanup. (osx !)
  Skipping redirection backing cleanup for $TESTTMP/direct_missing_config because its checkout config is missing. Run `eden du --clean-orphaned` to reclaim leftover backing storage. (no-osx !)
  Unmounting redirections...
  Error unmounting redirections for $TESTTMP/direct_missing_config: *config.toml does not exist* (glob)
  Unmounting checkout...
  Deleting EdenFS checkout state...
  removed $TESTTMP/direct_missing_config
  $ test -d $TESTTMP/direct_missing_config
  [1]
#endif

#if linux
test worktree remove - direct removal empties backing when a redirection can't be removed
(macOS removes the redirection symlink even though the checkout root is read-only.)

  $ sl worktree add $TESTTMP/direct_stuck_redirection
  created linked worktree at $TESTTMP/direct_stuck_redirection
  $ eden redirect add --mount $TESTTMP/direct_stuck_redirection scratch symlink
  $ backing="$(readlink $TESTTMP/direct_stuck_redirection/scratch)"
  $ mkdir "$backing/nested"
  $ echo contents > "$backing/nested/contents"
  $ chmod a-w $TESTTMP/direct_stuck_redirection
  $ sl worktree remove $TESTTMP/direct_stuck_redirection -y > $TESTTMP/stuck-remove.log 2>&1
  $ python -c 'import pathlib, sys; log = pathlib.Path(sys.argv[-1]).read_text(); print("Error unmounting redirections for" in log and "failed to unmount redirection scratch" in log)' $TESTTMP/stuck-remove.log
  True
  $ test -d $TESTTMP/direct_stuck_redirection
  [1]
  $ test -d "$backing"
  $ python -c 'import os, sys; print(os.listdir(sys.argv[-1]))' "$backing"
  []
#endif

re-create linked worktrees for remaining tests

  $ sl worktree add $TESTTMP/linked1
  created linked worktree at $TESTTMP/linked1
  $ sl worktree add $TESTTMP/linked2 --label "feature-x"
  created linked worktree at $TESTTMP/linked2

test worktree remove --all

  $ sl worktree remove --all -y
  Unmounting checkout...
  Deleting EdenFS checkout state...
  Unmounting checkout...
  Deleting EdenFS checkout state...
  removed $TESTTMP/linked1
  removed $TESTTMP/linked2

test worktree remove - group dissolved after all linked removed

  $ sl worktree list
  this worktree is not part of a group

test worktree remove - names the target when this checkout has no linked worktrees

  $ sl worktree remove $TESTTMP/linked1 -y
  abort: cannot remove $TESTTMP/linked1: $TESTTMP/myrepo has no linked worktrees
  [255]

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
  $ setconfig worktree.direct-eden-removal=false
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
  Unmounting checkout...
  Deleting EdenFS checkout state...
  removed $TESTTMP/pre_hook_rm1
#else
  $ setconfig hooks.pre-worktree-remove="echo PATH:\$HG_PATH"
  $ sl worktree remove $TESTTMP/pre_hook_rm1 -y
  PATH:$TESTTMP/pre_hook_rm1
  Unmounting checkout...
  Deleting EdenFS checkout state...
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
  Unmounting checkout...
  Deleting EdenFS checkout state...
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
  Unmounting checkout...
  Deleting EdenFS checkout state...
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
