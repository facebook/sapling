
#require eden

  $ setconfig worktree.enabled=true

setup backing repo with linked worktrees

  $ newclientrepo myrepo
  $ touch file.txt
  $ hg add file.txt
  $ hg commit -m "init"
  $ hg worktree add $TESTTMP/label_wt1
  created linked worktree at $TESTTMP/label_wt1
  $ hg worktree add $TESTTMP/label_wt2
  created linked worktree at $TESTTMP/label_wt2

test worktree label - positional TEXT (sets on current)

  $ hg worktree label "main-label"
  label set for $TESTTMP/myrepo

test worktree label - PATH TEXT (sets on specific worktree)

  $ hg worktree label $TESTTMP/label_wt1 "wt1-label"
  label set for $TESTTMP/label_wt1

test worktree label - --label flag

  $ hg worktree label $TESTTMP/label_wt2 --label "wt2-label"
  label set for $TESTTMP/label_wt2

test worktree label - verify in list output

  $ hg worktree list
    linked  $TESTTMP/label_wt1   wt1-label
    linked  $TESTTMP/label_wt2   wt2-label
  * main    $TESTTMP/myrepo   main-label

test worktree label - verify in JSON

  $ hg worktree list -Tjson | hg debugpython -- -c "
  > import json, sys
  > data = json.load(sys.stdin)
  > for e in data:
  >     print(e.get('label', None))
  > "
  wt1-label
  wt2-label
  main-label

test worktree label - --remove (removes from current)

  $ hg worktree label --remove
  label removed for $TESTTMP/myrepo
  $ hg worktree list
    linked  $TESTTMP/label_wt1   wt1-label
    linked  $TESTTMP/label_wt2   wt2-label
  * main    $TESTTMP/myrepo

test worktree label - PATH --remove

  $ hg worktree label $TESTTMP/label_wt1 --remove
  label removed for $TESTTMP/label_wt1

test worktree label - no args error

  $ hg worktree label
  abort: usage: sl worktree label [PATH] TEXT
  [255]

test worktree label - both positional and --label error

  $ hg worktree label $TESTTMP/label_wt2 "pos-label" --label "flag-label"
  abort: cannot specify both positional TEXT and --label
  [255]

test worktree label - --label with --remove error

  $ hg worktree label --label "x" --remove
  abort: --label cannot be used with --remove
  [255]
