#testcases nostackpush stackpush
  $ enable obsstore pushrebase amend

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > [mutation]
  > record=true
  > enabled=true
  > date=0 0
  > EOF

#if nostackpush
  $ setconfig pushrebase.trystackpush=false
#endif
#if stackpush
  $ setconfig pushrebase.trystackpush=true
#endif

Set up server repository

  $ hg init server
  $ cd server
  $ echo 1 > a
  $ echo 2 > b
  $ hg commit -Aqm base

Set up client repository

  $ cd ..
  $ hg clone ssh://user@dummy/server client -q

Add more commits on the server

  $ cd server
  $ echo 3 > c
  $ hg commit -Aqm s1
  $ echo 4 > d
  $ hg commit -Aqm s2

Pushrebase some commits from the client

  $ cd ../client
  $ echo 5 > e
  $ hg commit -Aqm c1
  $ echo 6 > f
  $ hg commit -Aqm c2
  $ echo 6a > f
  $ hg amend -qm "c2 (amended)"
  $ tglogp
  @  3: 62b5698bc9fd draft 'c2 (amended)'
  |
  o  1: b0c40d8745c8 draft 'c1'
  |
  o  0: a7d6a32ae4ec public 'base'
  
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 2 changesets:
  remote:     b0c40d8745c8  c1
  remote:     62b5698bc9fd  c2 (amended)
  remote: 4 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 2 changes to 4 files (+1 heads)
  2 new obsolescence markers
  obsoleted 2 changesets
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglogp
  @  7: 5a65089b6237 public 'c2 (amended)'
  |
  o  6: 2c9436cd8245 public 'c1'
  |
  o  5: 1f850c9f0d59 public 's2'
  |
  o  4: 06569a64c141 public 's1'
  |
  o  0: a7d6a32ae4ec public 'base'
  
  $ hg debugmutation ::tip
    a7d6a32ae4ecf473d6f934e731f1868dda4d3fc9
    06569a64c14156339463c64337f9cb5dc3a25442
    1f850c9f0d599261fce148d3d19cdc89d8eb391f
    2c9436cd8245ed8c8859cc3e4d3dd5084c51f1d4 pushrebase by test at 1970-01-01T00:00:00 from:
      b0c40d8745c83226015263d45e60a0d12722c515
    5a65089b6237971760ecf63669c9054b8e9bcdb9 pushrebase by test at 1970-01-01T00:00:00 from:
      62b5698bc9fdd97bfd09f5f4c681898396fcb4b5 amend by test at 1970-01-01T00:00:00 from:
        f558c5855324eea33b5f046b45b85db1fb98bca7

  $ cd ../server
  $ tglogp
  o  4: 5a65089b6237 public 'c2 (amended)'
  |
  o  3: 2c9436cd8245 public 'c1'
  |
  @  2: 1f850c9f0d59 public 's2'
  |
  o  1: 06569a64c141 public 's1'
  |
  o  0: a7d6a32ae4ec public 'base'
  
  $ hg debugmutation ::tip
    a7d6a32ae4ecf473d6f934e731f1868dda4d3fc9
    06569a64c14156339463c64337f9cb5dc3a25442
    1f850c9f0d599261fce148d3d19cdc89d8eb391f
    2c9436cd8245ed8c8859cc3e4d3dd5084c51f1d4 pushrebase by test at 1970-01-01T00:00:00 from:
      b0c40d8745c83226015263d45e60a0d12722c515
    5a65089b6237971760ecf63669c9054b8e9bcdb9 pushrebase by test at 1970-01-01T00:00:00 from:
      62b5698bc9fdd97bfd09f5f4c681898396fcb4b5
