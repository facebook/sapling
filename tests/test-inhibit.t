  $ cat >> $HGRCPATH <<EOF
  > [experimental]
  > evolution=createmarkers
  > [extensions]
  > drawdag=$RUNTESTDIR/drawdag.py
  > inhibit=$TESTDIR/../hgext3rd/inhibit.py
  > EOF

  $ hg init inhibit
  $ cd inhibit

  $ hg debugdrawdag <<'EOS'
  > B1 B2   # amend: B1 -> B2
  >  |/
  >  A
  > EOS

  $ hg up null -q
  $ hg log -T '{desc}' --hidden
  B2B1A (no-eol)

  $ B1=`HGPLAIN=1 hg log -r B1 -T '{node}' --hidden`
  $ B2=`HGPLAIN=1 hg log -r B2 -T '{node}' --hidden`

  $ hg debugobsolete $B2 $B1 -d '1 0'
  obsoleted 1 changesets
  $ hg log -G -T '{desc}' --hidden
  x  B2
  |
  | o  B1
  |/
  o  A
  
  $ hg debugobsolete $B1 $B2 -d '2 0'
  $ hg log -G -T '{desc}' --hidden
  o  B2
  |
  | x  B1
  |/
  o  A
  
  $ hg debugobsolete $B1 $B1 -d '3 0'
  $ hg log -G -T '{desc}' --hidden
  o  B2
  |
  | o  B1
  |/
  o  A
  
