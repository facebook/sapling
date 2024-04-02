#debugruntest-compatible

  $ configure mutation-norecord
#require rmcwd no-eden

Ensure that dirsync does not cause an abort when cwd goes missing

  $ enable rebase dirsync
  $ setconfig phases.publish=False

  $ configure modern
  $ newclientrepo
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
