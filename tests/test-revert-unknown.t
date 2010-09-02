  $ hg init
  $ touch unknown

  $ touch a
  $ hg add a
  $ hg ci -m "1"

  $ touch b
  $ hg add b
  $ hg ci -m "2"

Should show unknown

  $ hg status
  ? unknown
  $ hg revert -r 0 --all
  removing b

Should show unknown and b removed

  $ hg status
  R b
  ? unknown

Should show a and unknown

  $ ls
  a
  unknown
