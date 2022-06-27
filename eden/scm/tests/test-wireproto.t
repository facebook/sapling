#chg-compatible
  $ setconfig experimental.allowfilepeer=True

Test wire protocol argument passing

Setup repo:

  $ hg init repo

Local:

  $ hg debugwireargs repo eins zwei --three drei --four vier
  eins zwei drei vier None
  $ hg debugwireargs repo eins zwei --four vier
  eins zwei None vier None
  $ hg debugwireargs repo eins zwei
  eins zwei None None None
  $ hg debugwireargs repo eins zwei --five fuenf
  eins zwei None None fuenf
