Test uncommit with merges - set up the config

  $ cat >> $HGRCPATH <<EOF
  > [experimental]
  > evolution=createmarkers
  > [extensions]
  > uncommit = $TESTDIR/../hgext3rd/uncommit.py
  > EOF

Build up a repo

  $ hg init repo
  $ cd repo

Create some history

  $ touch a
  $ hg add a
  $ for i in 1 2 3 4 5; do echo $i > a; hg commit -m "a $i"; done
  $ hg checkout 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch b
  $ hg add b
  $ for i in 1 2 3 4 5; do echo $i > b; hg commit -m "b $i"; done
  created new head
  $ hg log -G -T '{rev}:{node} {desc}' --hidden
  @  9:6d48ed79c7c5f1f8384fb539cefc2f1a1875f945 b 5
  |
  o  8:34eb94a958c8011fdf761da547ec232eb3a31f40 b 4
  |
  o  7:2cd56cdde163ded2fbb16ba2f918c96046ab0bf2 b 3
  |
  o  6:c3a0d5bb3b15834ffd2ef9ef603e93ec65cf2037 b 2
  |
  o  5:49bb009ca26078726b8870f1edb29fae8f7618f5 b 1
  |
  | o  4:878392ab7cd2abf8e055802e9803ca47349a9bca a 5
  | |
  | o  3:e7efeddb4ac0ca56163381a3044c24e63fea93b7 a 4
  | |
  | o  2:990982b7384266e691f1bc08ca36177adcd1c8a9 a 3
  | |
  | o  1:24d38e3cf160c7b6f5ffe82179332229886a6d34 a 2
  |/
  o  0:ea4e33293d4d274a2ba73150733c2612231f398c a 1
  
Add and expect uncommit to fail on both merge working dir and merge changeset

  $ hg merge 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg uncommit
  abort: cannot uncommit while merging
  [255]
  $ hg status
  M a
  $ hg commit -m 'merge a and b'
  $ hg uncommit
  abort: cannot uncommit merge changeset
  [255]
  $ hg status
  $ hg log -G -T '{rev}:{node} {desc}' --hidden
  @    10:a153774ccc7a95b731811dfaf594c83364c80db5 merge a and b
  |\
  | o  9:6d48ed79c7c5f1f8384fb539cefc2f1a1875f945 b 5
  | |
  | o  8:34eb94a958c8011fdf761da547ec232eb3a31f40 b 4
  | |
  | o  7:2cd56cdde163ded2fbb16ba2f918c96046ab0bf2 b 3
  | |
  | o  6:c3a0d5bb3b15834ffd2ef9ef603e93ec65cf2037 b 2
  | |
  | o  5:49bb009ca26078726b8870f1edb29fae8f7618f5 b 1
  | |
  o |  4:878392ab7cd2abf8e055802e9803ca47349a9bca a 5
  | |
  o |  3:e7efeddb4ac0ca56163381a3044c24e63fea93b7 a 4
  | |
  o |  2:990982b7384266e691f1bc08ca36177adcd1c8a9 a 3
  | |
  o |  1:24d38e3cf160c7b6f5ffe82179332229886a6d34 a 2
  |/
  o  0:ea4e33293d4d274a2ba73150733c2612231f398c a 1
  
