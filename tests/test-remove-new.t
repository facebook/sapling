test that 'hg commit' does not crash if the user removes a newly added file

  $ hg init
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m "commit #0"
  $ touch b
  $ hg add b
  $ rm b
  $ hg commit -A -m"comment #1"
  removing b
  nothing changed
  [1]
