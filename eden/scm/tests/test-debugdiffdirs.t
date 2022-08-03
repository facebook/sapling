  $ setconfig workingcopy.ruststatus=False
  $ configure modernclient

  $ newclientrepo
  $ touch file
  $ hg commit -Aqm initial
  $ mkdir dir
  $ touch dir/foo
  $ hg commit -Aqm dir-added
  $ hg debugdiffdirs -r .^ -r .
  A dir

  $ echo >> dir/foo
  $ hg commit -Aqm file-modify
  $ hg debugdiffdirs -r .^ -r .

  $ touch dir/bar
  $ hg commit -Aqm file-added
  $ hg debugdiffdirs -r .^ -r .
  M dir

  $ hg rm dir/bar
  $ hg commit -Aqm file-removed
  $ hg debugdiffdirs -r .^ -r .
  M dir

  $ mkdir dir/nested
  $ touch dir/nested/poo
  $ hg commit -Aqm nested-added
  $ hg debugdiffdirs -r .^ -r .
  M dir
  A dir/nested

  $ hg rm dir/nested/poo
  $ touch dir/nested
  $ hg commit -Aqm nested-replaced
  $ hg debugdiffdirs -r .^ -r .
  M dir
  R dir/nested

  $ hg rm dir/nested
  $ mkdir dir/nested
  $ touch dir/nested/poo
  $ hg commit -Aqm nested-replaced-reverse
  $ hg debugdiffdirs -r .^ -r .
  M dir
  A dir/nested

  $ hg rm dir/nested
  removing dir/nested/poo
  $ hg commit -Aqm nested-removed

  $ hg rm dir/foo
  $ hg commit -Aqm dir-removed
  $ hg debugdiffdirs -r .^ -r .
  R dir

  $ hg debugdiffdirs -r 'desc("file-removed")' -r .
  R dir

  $ hg debugdiffdirs -r 'desc("initial")' -r 'desc("file-removed")'
  A dir

  $ hg debugdiffdirs -r 'desc("initial")' -r .
