  $ setconfig extensions.treemanifest=!
#testcases nostackpush stackpush
  $ enable pushrebase amend
  $ setconfig experimental.evolution=
  $ setconfig visibility.enabled=true
  $ setconfig mutation.record=true mutation.enabled=true mutation.date="0 0"
  $ setconfig ui.ssh="python \"$RUNTESTDIR/dummyssh\""

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
  @  3: e52ebff26308 draft 'c2 (amended)'
  |
  o  1: b0c40d8745c8 draft 'c1'
  |
  o  0: a7d6a32ae4ec public 'base'
  
  $ hg push --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 2 changesets:
  remote:     b0c40d8745c8  c1
  remote:     e52ebff26308  c2 (amended)
  remote: 4 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 2 changes to 4 files (+1 heads)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglogp
  @  7: bc165ecd11df public 'c2 (amended)'
  |
  o  6: 466bbcaf803c public 'c1'
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
    466bbcaf803c40b7121013141b842e654ee07f7f pushrebase by test at 1970-01-01T00:00:00 (synthetic) from:
      b0c40d8745c83226015263d45e60a0d12722c515
    bc165ecd11df56066a4d73e8294a85ecb255d3cf pushrebase by test at 1970-01-01T00:00:00 (synthetic) from:
      e52ebff2630810cbc8bc0e3a8de78cb662f0865f amend by test at 1970-01-01T00:00:00 from:
        f558c5855324eea33b5f046b45b85db1fb98bca7

  $ cd ../server
  $ tglogp
  o  4: bc165ecd11df public 'c2 (amended)'
  |
  o  3: 466bbcaf803c public 'c1'
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
    466bbcaf803c40b7121013141b842e654ee07f7f pushrebase by test at 1970-01-01T00:00:00 from:
      b0c40d8745c83226015263d45e60a0d12722c515
    bc165ecd11df56066a4d73e8294a85ecb255d3cf pushrebase by test at 1970-01-01T00:00:00 from:
      e52ebff2630810cbc8bc0e3a8de78cb662f0865f

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
  remote:     5cfa12ac15ac  c3

  $ hg debugmutation .
    5cfa12ac15aca3668b5f91e5a7b92aa309b320a9

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
  @  10: 254a42c0dcef draft 'c4 (amended)'
  |
  o  8: 5cfa12ac15ac public 'c3'
  |
  o  7: bc165ecd11df public 'c2 (amended)'
  |
  o  6: 466bbcaf803c public 'c1'
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
  remote:     254a42c0dcef  c4 (amended)
  remote: 3 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 3 files (+1 heads)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg debugmutation ".~4::."
    bc165ecd11df56066a4d73e8294a85ecb255d3cf pushrebase by test at 1970-01-01T00:00:00 (synthetic) from:
      e52ebff2630810cbc8bc0e3a8de78cb662f0865f amend by test at 1970-01-01T00:00:00 from:
        f558c5855324eea33b5f046b45b85db1fb98bca7
    5cfa12ac15aca3668b5f91e5a7b92aa309b320a9
    34295f2adc0954d129b43d9ad2d785376eacc3b6
    b6dffa66e38820804c5eaf4d2c9477718f537ce3
    56ff167c1749dc765639745247323a6139cd9514 pushrebase by test at 1970-01-01T00:00:00 (synthetic) from:
      254a42c0dcef8381419add47e4f0ff6cd50ea8c7 amend by test at 1970-01-01T00:00:00 from:
        3f1b3b3d517fcd3c8cef763476c588fb99343c3d

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
  remote:     6b21e03c2693  c5 (amended)
  remote: 2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglogp
  @  17: 7a5f07a2de1e public 'c5 (amended)'
  |
  o  16: 4c6ed9931ef5 public 's5'
  |
  o  13: 56ff167c1749 public 'c4 (amended)'
  |
  o  12: b6dffa66e388 public 's4'
  |
  o  11: 34295f2adc09 public 's3'
  |
  o  8: 5cfa12ac15ac public 'c3'
  |
  o  7: bc165ecd11df public 'c2 (amended)'
  |
  o  6: 466bbcaf803c public 'c1'
  |
  o  5: 1f850c9f0d59 public 's2'
  |
  o  4: 06569a64c141 public 's1'
  |
  o  0: a7d6a32ae4ec public 'base'
  
  $ hg debugmutation .
    7a5f07a2de1e5def6fa3288bde454239ed183ac1 pushrebase by test at 1970-01-01T00:00:00 (synthetic) from:
      6b21e03c2693b7ccaea8bbc2ed465bf0f20669ea amend by test at 1970-01-01T00:00:00 from:
        9407986f3421a1339098ddb424e7f4652626e70d

  $ tglogm --hidden
  @  17: 7a5f07a2de1e 'c5 (amended)'
  |
  o  16: 4c6ed9931ef5 's5'
  |
  | x  15: 6b21e03c2693 'c5 (amended)'  (Rewritten using pushrebase into 7a5f07a2de1e)
  |/
  | x  14: 9407986f3421 'c5'  (Rewritten using amend into 6b21e03c2693)
  |/
  o  13: 56ff167c1749 'c4 (amended)'
  |
  o  12: b6dffa66e388 's4'
  |
  o  11: 34295f2adc09 's3'
  |
  | x  10: 254a42c0dcef 'c4 (amended)'  (Rewritten using pushrebase into 56ff167c1749)
  |/
  | x  9: 3f1b3b3d517f 'c4'  (Rewritten using amend into 254a42c0dcef)
  |/
  o  8: 5cfa12ac15ac 'c3'
  |
  o  7: bc165ecd11df 'c2 (amended)'
  |
  o  6: 466bbcaf803c 'c1'
  |
  o  5: 1f850c9f0d59 's2'
  |
  o  4: 06569a64c141 's1'
  |
  | x  3: e52ebff26308 'c2 (amended)'  (Rewritten using pushrebase into bc165ecd11df)
  | |
  | | x  2: f558c5855324 'c2'  (Rewritten using amend into e52ebff26308)
  | |/
  | x  1: b0c40d8745c8 'c1'  (Rewritten using pushrebase into 466bbcaf803c)
  |/
  o  0: a7d6a32ae4ec 'base'
  
