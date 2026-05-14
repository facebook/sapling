
#require eden

  $ setconfig worktree.enabled=true

setup backing repo with linked worktrees

  $ newclientrepo myrepo
  $ touch file.txt
  $ sl add file.txt
  $ sl commit -m "init"
  $ sl worktree add $TESTTMP/label_wt1
  created linked worktree at $TESTTMP/label_wt1
  $ sl worktree add $TESTTMP/label_wt2
  created linked worktree at $TESTTMP/label_wt2

test worktree label - positional TEXT (sets on current)

  $ sl worktree label "main-label"
  label set for $TESTTMP/myrepo

test worktree label - PATH TEXT (sets on specific worktree)

  $ sl worktree label $TESTTMP/label_wt1 "wt1-label"
  label set for $TESTTMP/label_wt1

test worktree label - --label flag

  $ sl worktree label $TESTTMP/label_wt2 --label "wt2-label"
  label set for $TESTTMP/label_wt2

test worktree label - .sl/worktreename marker reflects each label

  $ cat $TESTTMP/myrepo/.sl/worktreename
  main-label (no-eol)
  $ cat $TESTTMP/label_wt1/.sl/worktreename
  wt1-label (no-eol)
  $ cat $TESTTMP/label_wt2/.sl/worktreename
  wt2-label (no-eol)

test worktree label - verify in list output

  $ sl worktree list
    linked  $TESTTMP/label_wt1   wt1-label
    linked  $TESTTMP/label_wt2   wt2-label
  * main    $TESTTMP/myrepo   main-label

test worktree label - verify in JSON

  $ sl worktree list -Tjson | sl debugpython -- -c "
  > import json, sys
  > data = json.load(sys.stdin)
  > for e in data:
  >     print(e.get('label', None))
  > "
  wt1-label
  wt2-label
  main-label

test worktree label - --remove (removes from current)

  $ sl worktree label --remove
  label removed for $TESTTMP/myrepo
  $ sl worktree list
    linked  $TESTTMP/label_wt1   wt1-label
    linked  $TESTTMP/label_wt2   wt2-label
  * main    $TESTTMP/myrepo

test worktree label - --remove falls back marker to worktree basename

  $ cat $TESTTMP/myrepo/.sl/worktreename
  myrepo (no-eol)

test worktree label - PATH --remove

  $ sl worktree label $TESTTMP/label_wt1 --remove
  label removed for $TESTTMP/label_wt1
  $ cat $TESTTMP/label_wt1/.sl/worktreename
  label_wt1 (no-eol)

test worktree label - marker-write failure does not abort the command

Replace the marker file with a directory so `fs::write` fails with EISDIR.
The registry update should still land and the command should exit 0.

  $ rm $TESTTMP/label_wt1/.sl/worktreename
  $ mkdir $TESTTMP/label_wt1/.sl/worktreename
  $ sl worktree label $TESTTMP/label_wt1 --label "best-effort-label"
  failed to write worktree-name marker for $TESTTMP/label_wt1: * (glob)
  label set for $TESTTMP/label_wt1
  $ sl worktree list
    linked  $TESTTMP/label_wt1   best-effort-label
    linked  $TESTTMP/label_wt2   wt2-label
  * main    $TESTTMP/myrepo
  $ rmdir $TESTTMP/label_wt1/.sl/worktreename

test worktree label - no args error

  $ sl worktree label
  abort: usage: sl worktree label [PATH] TEXT
  [255]

test worktree label - both positional and --label error

  $ sl worktree label $TESTTMP/label_wt2 "pos-label" --label "flag-label"
  abort: cannot specify both positional TEXT and --label
  [255]

test worktree label - --label with --remove error

  $ sl worktree label --label "x" --remove
  abort: --label cannot be used with --remove
  [255]
