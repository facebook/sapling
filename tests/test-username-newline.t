  $ hg init
  $ touch a

  $ unset HGUSER
  $ echo "[ui]" >> .hg/hgrc
  $ echo "username= foo" >> .hg/hgrc
  $ echo "          bar1" >> .hg/hgrc

  $ hg ci -Am m
  adding a
  abort: username 'foo\nbar1' contains a newline
  
  [255]
  $ rm .hg/hgrc

  $ HGUSER=`(echo foo; echo bar2)` hg ci -Am m
  abort: username 'foo\nbar2' contains a newline
  
  [255]
  $ hg ci -Am m -u "`(echo foo; echo bar3)`"
  transaction abort!
  rollback completed
  abort: username 'foo\nbar3' contains a newline!
  [255]

