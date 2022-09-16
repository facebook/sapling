#debugruntest-compatible
#chg-compatible

  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ configure mutation-norecord
#require rmcwd

Ensure that dirsync does not cause an abort when cwd goes missing

  $ enable rebase dirsync
  $ setconfig phases.publish=False

  $ newrepo
  $ drawdag <<'EOF'
  >   change    # change/a = a
  >    |
  >    | delete # delete/dir/a = (removed)
  >    | /
  >   base      # base/dir/a = a
  > EOF

  $ hg co -q $change
  $ cd dir

  $ hg rebase -s . -d $delete
  rebasing * "change" (glob)
  current directory was removed
  (consider changing to repo root: $TESTTMP/repo1)

  $ cd "$TESTTMP/repo1"
  $ hg status

  $ hg log -Gr "all()" -T "{node|short} {desc}"
  @  * change (glob)
  │
  o  * delete (glob)
  │
  o  * base (glob)
  
  $ hg rebase --abort
  abort: no rebase in progress
  [255]
