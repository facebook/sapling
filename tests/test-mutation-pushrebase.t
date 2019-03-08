#testcases nostackpush stackpush
  $ enable pushrebase amend
  $ setconfig experimental.evolution=

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > [mutation]
  > record=true
  > enabled=true
  > date=0 0
  > [visibility]
  > tracking=on
  > [templatealias]
  > sl_mutation_names = dict(amend="Amended as", rebase="Rebased to", split="Split into", fold="Folded into", histedit="Histedited to", rewrite="Rewritten into", land="Landed as", pushrebase="Pushed as")
  > sl_mutations = "{join(mutations % '({get(sl_mutation_names, operation, "Rewritten using {operation} into")} {join(successors % "{node|short}", ", ")})', ' ')}"
  > sl_mutation_descs = "{join(mutations % '({get(sl_mutation_names, operation, "Rewritten using {operation} into")} {join(successors % "{desc}", ", ")})', ' ')}"
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

Test pushing to a server that does not have mutation recording enabled.  Synthetic mutation
entries will be contructed from the obsmarkers that pushrebase returns.

  $ cd ../server
  $ cat >> .hg/hgrc <<EOF
  > [mutation]
  > record=false
  > enabled=false
  > EOF

Push an original commit to the server.  This doesn't get pushrebased.

  $ cd ../client
  $ echo 9 > i
  $ hg commit -Aqm c3
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 changeset:
  remote:     b412b1404866  c3

  $ hg debugmutation .
    b412b1404866f535babf346c79d1f998cb3fb0e9

Add commits on the server to pushrebase over.

  $ cd ../server
  $ hg up -q tip
  $ echo 7 > g
  $ hg commit -Aqm s3
  $ echo 8 > h
  $ hg commit -Aqm s4

Add another commit on the client.

  $ cd ../client
  $ echo 10 > j
  $ hg commit -Aqm c4
  $ echo 10a > j
  $ hg amend -qm "c4 (amended)"
  $ tglogp
  @  10: 99022e9e8280 draft 'c4 (amended)'
  |
  o  8: b412b1404866 public 'c3'
  |
  o  7: 5a65089b6237 public 'c2 (amended)'
  |
  o  6: 2c9436cd8245 public 'c1'
  |
  o  5: 1f850c9f0d59 public 's2'
  |
  o  4: 06569a64c141 public 's1'
  |
  o  0: a7d6a32ae4ec public 'base'
  
Push this commit to the server.  We should create local mutation information.

  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 changeset:
  remote:     99022e9e8280  c4 (amended)
  remote: 3 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 3 files (+1 heads)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg debugmutation ".~4::."
    5a65089b6237971760ecf63669c9054b8e9bcdb9 pushrebase by test at 1970-01-01T00:00:00 from:
      62b5698bc9fdd97bfd09f5f4c681898396fcb4b5 amend by test at 1970-01-01T00:00:00 from:
        f558c5855324eea33b5f046b45b85db1fb98bca7
    b412b1404866f535babf346c79d1f998cb3fb0e9
    6ac3dc55390fd3e54a76605120952251a38b0d03
    89faaa686260f086606967431708d81ae32fd514
    c2f349ee44e38f4506278e28307a16c076fa2804 pushrebase by test at 1970-01-01T00:00:00 (synthetic) from:
      99022e9e8280d27b807c9ffe5867a140486f437e amend by test at 1970-01-01T00:00:00 from:
        b1917feb04ec09fbf553132a84c94391d7c99d74

Test pushing to a futuristic server that doesn't support obsmarkers at all will still behave correctly.

  $ cd ../server
  $ cat >> .hg/hgrc << EOF
  > [mutation]
  > record=true
  > enabled=true
  > [pushrebase]
  > pushback.obsmarkers=false
  > EOF
  $ hg up -q tip
  $ echo 11 > k
  $ hg commit -Aqm s5

  $ cd ../client
  $ echo 12 > l
  $ hg commit -Aqm c5
  $ echo 12a > l
  $ hg amend -qm "c5 (amended)"
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 changeset:
  remote:     36335f21bebe  c5 (amended)
  remote: 2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglogp
  @  17: b464f8cb0c77 public 'c5 (amended)'
  |
  o  16: 56293e576988 public 's5'
  |
  o  13: c2f349ee44e3 public 'c4 (amended)'
  |
  o  12: 89faaa686260 public 's4'
  |
  o  11: 6ac3dc55390f public 's3'
  |
  o  8: b412b1404866 public 'c3'
  |
  o  7: 5a65089b6237 public 'c2 (amended)'
  |
  o  6: 2c9436cd8245 public 'c1'
  |
  o  5: 1f850c9f0d59 public 's2'
  |
  o  4: 06569a64c141 public 's1'
  |
  o  0: a7d6a32ae4ec public 'base'
  
  $ hg debugmutation .
    b464f8cb0c777ddac0b42fe293e8d2d2129a311d pushrebase by test at 1970-01-01T00:00:00 from:
      36335f21bebe13bdad75c02dfcc260c92ddbb6cb amend by test at 1970-01-01T00:00:00 from:
        1c5bc87fa98b4dff8d1d8c96d99ab787f781f7d1

  $ hg log -G -T '{node|short} {desc} {sl_mutations}' --hidden
  @  b464f8cb0c77 c5 (amended)
  |
  o  56293e576988 s5
  |
  | x  36335f21bebe c5 (amended) (Pushed as b464f8cb0c77)
  |/
  | x  1c5bc87fa98b c5 (Amended as 36335f21bebe)
  |/
  o  c2f349ee44e3 c4 (amended)
  |
  o  89faaa686260 s4
  |
  o  6ac3dc55390f s3
  |
  | x  99022e9e8280 c4 (amended) (Pushed as c2f349ee44e3)
  |/
  | x  b1917feb04ec c4 (Amended as 99022e9e8280)
  |/
  o  b412b1404866 c3
  |
  o  5a65089b6237 c2 (amended)
  |
  o  2c9436cd8245 c1
  |
  o  1f850c9f0d59 s2
  |
  o  06569a64c141 s1
  |
  | x  62b5698bc9fd c2 (amended) (Pushed as 5a65089b6237)
  | |
  | | x  f558c5855324 c2 (Amended as 62b5698bc9fd)
  | |/
  | x  b0c40d8745c8 c1 (Pushed as 2c9436cd8245)
  |/
  o  a7d6a32ae4ec base
  
