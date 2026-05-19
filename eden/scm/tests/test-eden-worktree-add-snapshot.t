
#require eden

  $ setconfig worktree.enabled=true worktree.snapshot-direct-copy=true

setup backing repo

  $ newclientrepo myrepo
  $ echo base > file.txt
  $ mkdir -p dir/subdir
  $ echo nested > dir/subdir/nested.txt
  $ sl add file.txt dir/subdir/nested.txt
  $ sl commit -m "init"

test worktree add --snapshot - modified file

  $ echo modified > file.txt
  $ sl worktree add --snapshot $TESTTMP/wt_modified
  computing working copy status...
  created linked worktree at $TESTTMP/wt_modified
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ cat $TESTTMP/wt_modified/file.txt
  modified
  $ cd $TESTTMP/wt_modified && sl status
  M file.txt
  $ cd $TESTTMP/myrepo
  $ sl revert --all --no-backup -q

test worktree add --snapshot - added file (sl add)

  $ echo added > new_file.txt
  $ sl add new_file.txt
  $ sl worktree add --snapshot $TESTTMP/wt_added
  computing working copy status...
  created linked worktree at $TESTTMP/wt_added
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ cat $TESTTMP/wt_added/new_file.txt
  added
  $ cd $TESTTMP/wt_added && sl status
  A new_file.txt
  $ cd $TESTTMP/myrepo
  $ sl revert --all --no-backup -q && rm -f new_file.txt

test worktree add --snapshot - untracked file

  $ echo untracked > untracked.txt
  $ sl worktree add --snapshot $TESTTMP/wt_untracked
  computing working copy status...
  created linked worktree at $TESTTMP/wt_untracked
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ cat $TESTTMP/wt_untracked/untracked.txt
  untracked
  $ cd $TESTTMP/wt_untracked && sl status
  ? untracked.txt
  $ cd $TESTTMP/myrepo
  $ rm untracked.txt

test worktree add --snapshot - removed file (sl rm)

  $ sl rm file.txt
  $ sl worktree add --snapshot $TESTTMP/wt_removed
  computing working copy status...
  created linked worktree at $TESTTMP/wt_removed
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ test -f $TESTTMP/wt_removed/file.txt
  [1]
  $ cd $TESTTMP/wt_removed && sl status
  R file.txt
  $ cd $TESTTMP/myrepo
  $ sl revert --all --no-backup -q

test worktree add --snapshot - nested directory with new untracked files

  $ mkdir -p newdir/deep/nested
  $ echo a > newdir/deep/nested/a.txt
  $ echo b > newdir/deep/b.txt
  $ sl worktree add --snapshot $TESTTMP/wt_nested
  computing working copy status...
  created linked worktree at $TESTTMP/wt_nested
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ cat $TESTTMP/wt_nested/newdir/deep/nested/a.txt
  a
  $ cat $TESTTMP/wt_nested/newdir/deep/b.txt
  b
  $ cd $TESTTMP/wt_nested && sl status | sort
  ? newdir/deep/b.txt
  ? newdir/deep/nested/a.txt
  $ cd $TESTTMP/myrepo
  $ rm -rf newdir

test worktree add --snapshot - mixed changes (all types at once)

  $ echo modified_again > file.txt
  $ echo brand_new > added.txt && sl add added.txt
  $ echo just_here > floating.txt
  $ sl rm dir/subdir/nested.txt
  $ sl worktree add --snapshot $TESTTMP/wt_mixed
  computing working copy status...
  created linked worktree at $TESTTMP/wt_mixed
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ cat $TESTTMP/wt_mixed/file.txt
  modified_again
  $ cat $TESTTMP/wt_mixed/added.txt
  brand_new
  $ cat $TESTTMP/wt_mixed/floating.txt
  just_here
  $ test -f $TESTTMP/wt_mixed/dir/subdir/nested.txt
  [1]
  $ cd $TESTTMP/wt_mixed && sl status | sort
  ? floating.txt
  A added.txt
  M file.txt
  R dir/subdir/nested.txt
  $ cd $TESTTMP/myrepo
  $ sl revert --all --no-backup -q && rm -f added.txt floating.txt

#if no-windows
test worktree add --snapshot - executable file

  $ echo '#!/bin/sh' > script.sh
  $ chmod +x script.sh
  $ sl worktree add --snapshot $TESTTMP/wt_exec
  computing working copy status...
  created linked worktree at $TESTTMP/wt_exec
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ test -x $TESTTMP/wt_exec/script.sh
  $ cat $TESTTMP/wt_exec/script.sh
  #!/bin/sh
  $ cd $TESTTMP/myrepo
  $ rm script.sh
#endif

#if no-windows
test worktree add --snapshot - symlink

  $ ln -s file.txt link.txt
  $ sl worktree add --snapshot $TESTTMP/wt_symlink
  computing working copy status...
  created linked worktree at $TESTTMP/wt_symlink
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ readlink $TESTTMP/wt_symlink/link.txt
  file.txt
  $ cd $TESTTMP/myrepo
  $ rm link.txt
#endif

test worktree add --snapshot - deleted (missing) file

  $ rm file.txt
  $ sl status
  ! file.txt
  $ sl worktree add --snapshot $TESTTMP/wt_deleted
  computing working copy status...
  created linked worktree at $TESTTMP/wt_deleted
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ test -f $TESTTMP/wt_deleted/file.txt
  [1]
  $ cd $TESTTMP/wt_deleted && sl status
  ! file.txt
  $ cd $TESTTMP/myrepo
  $ sl revert --all --no-backup -q

#if no-windows
test worktree add --snapshot - file type change to symlink

  $ rm file.txt && ln -s dir/subdir/nested.txt file.txt
  $ readlink file.txt
  dir/subdir/nested.txt
  $ sl worktree add --snapshot $TESTTMP/wt_typechange
  computing working copy status...
  created linked worktree at $TESTTMP/wt_typechange
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ readlink $TESTTMP/wt_typechange/file.txt
  dir/subdir/nested.txt
  $ cd $TESTTMP/myrepo
  $ rm file.txt && sl revert --all --no-backup -q
#endif

test worktree add --snapshot - remove all files in a directory

  $ sl rm dir/subdir/nested.txt
  $ sl worktree add --snapshot $TESTTMP/wt_rmdir
  computing working copy status...
  created linked worktree at $TESTTMP/wt_rmdir
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ test -f $TESTTMP/wt_rmdir/dir/subdir/nested.txt
  [1]
  $ cd $TESTTMP/wt_rmdir && sl status
  R dir/subdir/nested.txt
  $ cd $TESTTMP/myrepo
  $ sl revert --all --no-backup -q

test worktree add --snapshot - file replaced by directory with added file

  $ sl rm file.txt
  $ mkdir file.txt
  $ echo inside > file.txt/child.txt
  $ sl add file.txt/child.txt
  $ sl status | sort
  A file.txt/child.txt
  R file.txt
  $ sl worktree add --snapshot $TESTTMP/wt_file_to_dir
  computing working copy status...
  created linked worktree at $TESTTMP/wt_file_to_dir
  applying working copy changes to new worktree...
  working copy changes applied to new worktree
  $ test -f $TESTTMP/wt_file_to_dir/file.txt/child.txt
  $ cat $TESTTMP/wt_file_to_dir/file.txt/child.txt
  inside
  $ cd $TESTTMP/wt_file_to_dir && sl status | sort
  A file.txt/child.txt
  R file.txt
  $ cd $TESTTMP/myrepo
  $ sl revert --all --no-backup -q && rm -rf file.txt && sl revert --all --no-backup -q

test worktree add --snapshot - merge in progress (two parents)
neither direct copy nor the legacy snapshot path preserves p2 or merge state,
so we bail out early.

  $ sl goto -C 'desc(init)' -q
  $ echo branch1 > branch.txt
  $ sl add branch.txt
  $ sl commit -m "branch1"
  $ sl goto -C 'desc(init)' -q
  $ echo branch2 > branch.txt
  $ sl add branch.txt
  $ sl commit -m "branch2"
  $ sl debugsetparents . 'desc(branch1)'
  $ echo dirty > file.txt
verify source has two parents
  $ sl log -r . -T '{node|short}\n'
  * (glob)
  $ sl log -r 'parents()' -T '{desc}\n'
  branch1
  branch2
  $ sl worktree add --snapshot $TESTTMP/wt_merge
  abort: working copy has two parents; snapshot cannot preserve merge state
  [255]
  $ cd $TESTTMP/myrepo
  $ sl debugsetparents .
  $ sl goto -C 'desc(init)' -q

test worktree add --snapshot - clean working copy (no changes to copy)

  $ sl worktree add --snapshot $TESTTMP/wt_clean
  computing working copy status...
  created linked worktree at $TESTTMP/wt_clean
  working copy is clean, nothing to copy
  $ cd $TESTTMP/wt_clean && sl status
  $ cd $TESTTMP/myrepo

test worktree add --snapshot - without --snapshot flag (no copy)

  $ echo dirty > file.txt
  $ sl worktree add $TESTTMP/wt_no_snapshot
  created linked worktree at $TESTTMP/wt_no_snapshot
  $ cat $TESTTMP/wt_no_snapshot/file.txt
  base
  $ cd $TESTTMP/myrepo
  $ sl revert --all --no-backup -q
