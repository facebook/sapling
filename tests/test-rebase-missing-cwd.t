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
  rebasing 1:6f52fdb3a577 "change"
  current directory was removed
  (consider changing to repo root: $TESTTMP/repo1)
  abort: $ENOENT$
  [255]

  $ cd -
  $TESTTMP/repo1
  $ hg status
  M a
  M change

  $ hg log -Gr ":" -T "{node|short} {desc}"
  @  5f45087392e8 delete
  |
  | @  6f52fdb3a577 change
  |/
  o  aa6caddcd04f base
  
  $ hg rebase --abort
  rebase aborted
