#chg-compatible

  $ for src in 0 1 2; do
  >   for dst in 0 1 2; do
  >     [ $src = $dst ] && continue
  >     echo ==== Migrating dirstate v$src to v$dst ====
  >     cd $TESTTMP
  >     setconfig format.dirstate=$src
  >     newrepo
  >     touch normal modified removed deleted
  >     hg ci -A . -q -m init
  >     hg rm removed
  >     rm deleted
  >     touch untracked
  >     echo 1 > modified
  >     hg status
  >     hg debugtreestate status
  >     hg debugtreestate v$dst
  >     hg status
  >     hg debugtreestate status
  >   done
  > done
  ==== Migrating dirstate v0 to v1 ====
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v0 (flat dirstate, 4 files tracked)
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v1 (using dirstate.tree.*, 4 files tracked) (glob)
  ==== Migrating dirstate v0 to v2 ====
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v0 (flat dirstate, 4 files tracked)
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v2 (using treestate*, offset *, 4 files tracked) (glob) (no-fsmonitor !)
  dirstate v2 (using treestate*, offset *, 5 files tracked) (glob) (fsmonitor !)
  ==== Migrating dirstate v1 to v0 ====
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v1 (using dirstate.tree*, 4 files tracked) (glob)
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v0 (flat dirstate, 4 files tracked)
  ==== Migrating dirstate v1 to v2 ====
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v1 (using dirstate.tree*, 4 files tracked) (glob)
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v2 (using treestate*, offset *, 4 files tracked) (glob) (no-fsmonitor !)
  dirstate v2 (using treestate*, offset *, 5 files tracked) (glob) (fsmonitor !)
  ==== Migrating dirstate v2 to v0 ====
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v2 (using treestate*, offset *, 4 files tracked) (glob) (no-fsmonitor !)
  dirstate v2 (using treestate*, offset *, 5 files tracked) (glob) (fsmonitor !)
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v0 (flat dirstate, 4 files tracked)
  ==== Migrating dirstate v2 to v1 ====
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v2 (using treestate*, offset *, 4 files tracked) (glob) (no-fsmonitor !)
  dirstate v2 (using treestate*, offset *, 5 files tracked) (glob) (fsmonitor !)
  M modified
  R removed
  ! deleted
  ? untracked
  dirstate v1 (using dirstate.tree*, 4 files tracked) (glob)
