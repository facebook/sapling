#chg-compatible

  $ newrepo
  $ enable smartlog
  $ setconfig ui.allowemptycommit=1 phases.publish=False smartlog.master=master
  $ drawdag << 'EOS'
  > B C  # B has date 100000 0
  > |/   # C has date 200000 0
  > A
  > EOS
  $ hg bookmark -ir $A master
  $ hg sl --config smartlog.hide-before='10000 0' -T '{desc}'
  o  C
  |
  | o  B
  |/
  o  A
  
  $ hg sl --config smartlog.hide-before='150000 0' -T '{desc}'
  o  C
  |
  o  A
  
  note: hiding 1 old heads without bookmarks
  (use --all to see them)
  $ hg sl --config smartlog.hide-before='250000 0' -T '{desc}'
  o  A
  
  note: hiding 2 old heads without bookmarks
  (use --all to see them)
