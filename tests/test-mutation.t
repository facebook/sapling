  $ enable amend rebase histedit fbhistedit

We need obsmarkers for now, to allow unstable commits
  $ enable obsstore

  $ cat >> $HGRCPATH <<EOF
  > [mutation]
  > record=true
  > date=0 0
  > [ui]
  > interactive = true
  > EOF
  $ newrepo
  $ echo "base" > base
  $ hg commit -Aqm base
  $ echo "1" > file
  $ hg commit -Aqm c1

Amend

  $ for i in 2 3 4 5 6 7 8
  > do
  >   echo $i >> file
  >   hg amend -m "c1 (amended $i)"
  > done
  $ hg debugmutation .
    c5fb4c2b7fcf4b995e8cd8f6b0cb5186d9b5b935 amend by test at 1970-01-01T00:00:00 from:
      61fdcd12ad98987cfda8da08c8e4d69f63c5fd89 amend by test at 1970-01-01T00:00:00 from:
        661239d41405ed7e61d05a207ea470ba2a81b593 amend by test at 1970-01-01T00:00:00 from:
          ac4fa5bf18651efbc4aea658be1f662cf6957b52 amend by test at 1970-01-01T00:00:00 from:
            815e611f4a75e6752f30d74f243c48cdccf4bd1e amend by test at 1970-01-01T00:00:00 from:
              c8d40e41915aa2f98b88954ce404025953dbc12a amend by test at 1970-01-01T00:00:00 from:
                4c8af5bba994ede28e843f607374031db8abd043 amend by test at 1970-01-01T00:00:00 from:
                  c5d0fa8770bdde6ef311cc640a78a2f686be28b4

Rebase

  $ echo "a" > file2
  $ hg commit -Aqm c2
  $ echo "a" > file3
  $ hg commit -Aqm c3
  $ hg rebase -q -s ".^" -d 0
  $ hg rebase -q -s ".^" -d 1 --hidden
  $ hg rebase -q -s ".^" -d 8 --hidden
  $ hg debugmutation ".^::."
    ded4fa782bd8c1051c8be550cebbc267572e15d0 rebase by test at 1970-01-01T00:00:00 from:
      33905c5919f60e31c4e4f00ad5956a06848cbe10 rebase by test at 1970-01-01T00:00:00 from:
        afdb4ea72e8cb14b34dfae49b9cc9be698468edf rebase by test at 1970-01-01T00:00:00 from:
          561937d12f41e7d2f5ade2799de1bc21b92ddc51
    8462f4f357413f9f1c76a798d6ccdfc1e4337bd7 rebase by test at 1970-01-01T00:00:00 from:
      8ae4b2d33bbb804e1e8a5d5e43164e61dfb09885 rebase by test at 1970-01-01T00:00:00 from:
        afcbdd90543ac6273d77ce2b6e967fb73373e5a4 rebase by test at 1970-01-01T00:00:00 from:
          1e2c46af1a22b8949201aee655b53f2aba83c490

Metaedit

  $ hg meta -m "c3 (metaedited)"
  $ hg debugmutation .
    60f9e7d031c5b05f8ff106d39a20d67c40dc7411 metaedit by test at 1970-01-01T00:00:00 from:
      8462f4f357413f9f1c76a798d6ccdfc1e4337bd7 rebase by test at 1970-01-01T00:00:00 from:
        8ae4b2d33bbb804e1e8a5d5e43164e61dfb09885 rebase by test at 1970-01-01T00:00:00 from:
          afcbdd90543ac6273d77ce2b6e967fb73373e5a4 rebase by test at 1970-01-01T00:00:00 from:
            1e2c46af1a22b8949201aee655b53f2aba83c490

Fold

  $ hg fold --from ".^"
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugmutation .
    2fd85d288d1b25636df6532b000fbb150e43646e fold by test at 1970-01-01T00:00:00 from:
      ded4fa782bd8c1051c8be550cebbc267572e15d0 rebase by test at 1970-01-01T00:00:00 from:
        33905c5919f60e31c4e4f00ad5956a06848cbe10 rebase by test at 1970-01-01T00:00:00 from:
          afdb4ea72e8cb14b34dfae49b9cc9be698468edf rebase by test at 1970-01-01T00:00:00 from:
            561937d12f41e7d2f5ade2799de1bc21b92ddc51
      60f9e7d031c5b05f8ff106d39a20d67c40dc7411 metaedit by test at 1970-01-01T00:00:00 from:
        8462f4f357413f9f1c76a798d6ccdfc1e4337bd7 rebase by test at 1970-01-01T00:00:00 from:
          8ae4b2d33bbb804e1e8a5d5e43164e61dfb09885 rebase by test at 1970-01-01T00:00:00 from:
            afcbdd90543ac6273d77ce2b6e967fb73373e5a4 rebase by test at 1970-01-01T00:00:00 from:
              1e2c46af1a22b8949201aee655b53f2aba83c490

Split, leaving some changes left over at the end

  $ echo "b" >> file2
  $ echo "b" >> file3
  $ hg commit -qm c4
  $ hg split << EOF
  > y
  > y
  > n
  > y
  > EOF
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  reverting file2
  reverting file3
  diff --git a/file2 b/file2
  1 hunks, 1 lines changed
  examine changes to 'file2'? [Ynesfdaq?] y
  
  @@ -1,1 +1,2 @@
   a
  +b
  record change 1/2 to 'file2'? [Ynesfdaq?] y
  
  diff --git a/file3 b/file3
  1 hunks, 1 lines changed
  examine changes to 'file3'? [Ynesfdaq?] n
  
  Done splitting? [yN] y
  $ hg debugmutation ".^::."
    a7e46e8d9faf725274ea4cde6d202dd8d74991b0
    b23a10bc8972610ae489b044312b4e89e89fa08e split by test at 1970-01-01T00:00:00 (split into this and: a7e46e8d9faf725274ea4cde6d202dd8d74991b0) from:
      618c9a83fb832b6742123bd06fa829aa32bdb1bf

Split parent, selecting all changes at the end

  $ echo "c" >> file2
  $ echo "c" >> file3
  $ hg commit -qm c5
  $ echo "d" >> file3
  $ hg commit -qm c6
  $ hg split ".^" << EOF
  > y
  > y
  > n
  > n
  > y
  > y
  > EOF
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  reverting file2
  reverting file3
  diff --git a/file2 b/file2
  1 hunks, 1 lines changed
  examine changes to 'file2'? [Ynesfdaq?] y
  
  @@ -1,2 +1,3 @@
   a
   b
  +c
  record change 1/2 to 'file2'? [Ynesfdaq?] y
  
  diff --git a/file3 b/file3
  1 hunks, 1 lines changed
  examine changes to 'file3'? [Ynesfdaq?] n
  
  Done splitting? [yN] n
  diff --git a/file3 b/file3
  1 hunks, 1 lines changed
  examine changes to 'file3'? [Ynesfdaq?] y
  
  @@ -1,2 +1,3 @@
   a
   b
  +c
  record this change to 'file3'? [Ynesfdaq?] y
  
  no more change to split
  rebasing 23:2802b58ff916 "c6"

Split leaves the checkout at the top of the split commits

  $ hg debugmutation ".^::tip"
    9f5728118af072cb4d27b2e87c1c4abf1d744c54
    94fde643eeb6b11e10eb5de6268ce62601f8c185 split by test at 1970-01-01T00:00:00 (split into this and: 9f5728118af072cb4d27b2e87c1c4abf1d744c54) from:
      98372bb0c913529155d64663575faf5698fe8b1b
    e536de343881687fa51ea0174bd3333686cb4ced rebase by test at 1970-01-01T00:00:00 from:
      2802b58ff916d7dbca8462b9843ce7fca4ca18f4

Amend with rebase afterwards (split info should not be propagated)

  $ hg amend --rebase -m "c5 (split)"
  rebasing 26:e536de343881 "c6"
  $ hg debugmutation ".::tip"
    383692dec8a1036c5b62a49a9808738c5ab72075 amend by test at 1970-01-01T00:00:00 from:
      94fde643eeb6b11e10eb5de6268ce62601f8c185 split by test at 1970-01-01T00:00:00 (split into this and: 9f5728118af072cb4d27b2e87c1c4abf1d744c54) from:
        98372bb0c913529155d64663575faf5698fe8b1b
    d0b31d57fee70727f54b94594aec20afaa8bb34c rebase by test at 1970-01-01T00:00:00 from:
      e536de343881687fa51ea0174bd3333686cb4ced rebase by test at 1970-01-01T00:00:00 from:
        2802b58ff916d7dbca8462b9843ce7fca4ca18f4

Histedit

  $ . "$TESTDIR/histedit-helpers.sh"

  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "e" >> file4
  $ hg commit -Aqm c7
  $ echo "f" >> file4
  $ hg commit -Aqm c8
  $ echo "g" >> file4
  $ hg commit -Aqm c9
  $ hg histedit 8 --commands - 2>&1 <<EOF | fixbundle
  > pick c5fb4c2b7fcf
  > pick 2fd85d288d1b
  > fold a7e46e8d9faf
  > roll b23a10bc8972
  > fold 9f5728118af0
  > roll 383692dec8a1
  > pick d0b31d57fee7
  > roll c0807ccf7001
  > roll 7cc715a98301
  > pick 3df81c50780f
  > EOF
  $ hg debugmutation 8::tip
    c5fb4c2b7fcf4b995e8cd8f6b0cb5186d9b5b935 amend by test at 1970-01-01T00:00:00 from:
      61fdcd12ad98987cfda8da08c8e4d69f63c5fd89 amend by test at 1970-01-01T00:00:00 from:
        661239d41405ed7e61d05a207ea470ba2a81b593 amend by test at 1970-01-01T00:00:00 from:
          ac4fa5bf18651efbc4aea658be1f662cf6957b52 amend by test at 1970-01-01T00:00:00 from:
            815e611f4a75e6752f30d74f243c48cdccf4bd1e amend by test at 1970-01-01T00:00:00 from:
              c8d40e41915aa2f98b88954ce404025953dbc12a amend by test at 1970-01-01T00:00:00 from:
                4c8af5bba994ede28e843f607374031db8abd043 amend by test at 1970-01-01T00:00:00 from:
                  c5d0fa8770bdde6ef311cc640a78a2f686be28b4
    2a2702418db0647c75b35bffa75ad7b4ea377e44 histedit by test at 1970-01-01T00:00:00 from:
      16c4bfbbca18238ddc7bb3946a0b6b230464799b histedit by test at 1970-01-01T00:00:00 from:
        e086d79182ddf80b13bf03020e7955d523f78afc histedit by test at 1970-01-01T00:00:00 from:
          f9036a3722b2b4cdbd55d08cb6cba9a38bdd01a3 histedit by test at 1970-01-01T00:00:00 from:
            2fd85d288d1b25636df6532b000fbb150e43646e fold by test at 1970-01-01T00:00:00 from:
              ded4fa782bd8c1051c8be550cebbc267572e15d0 rebase by test at 1970-01-01T00:00:00 from:
                33905c5919f60e31c4e4f00ad5956a06848cbe10 rebase by test at 1970-01-01T00:00:00 from:
                  afdb4ea72e8cb14b34dfae49b9cc9be698468edf rebase by test at 1970-01-01T00:00:00 from:
                    561937d12f41e7d2f5ade2799de1bc21b92ddc51
              60f9e7d031c5b05f8ff106d39a20d67c40dc7411 metaedit by test at 1970-01-01T00:00:00 from:
                8462f4f357413f9f1c76a798d6ccdfc1e4337bd7 rebase by test at 1970-01-01T00:00:00 from:
                  8ae4b2d33bbb804e1e8a5d5e43164e61dfb09885 rebase by test at 1970-01-01T00:00:00 from:
                    afcbdd90543ac6273d77ce2b6e967fb73373e5a4 rebase by test at 1970-01-01T00:00:00 from:
                      1e2c46af1a22b8949201aee655b53f2aba83c490
            a7e46e8d9faf725274ea4cde6d202dd8d74991b0
          b23a10bc8972610ae489b044312b4e89e89fa08e split by test at 1970-01-01T00:00:00 (split into this and: a7e46e8d9faf725274ea4cde6d202dd8d74991b0) from:
            618c9a83fb832b6742123bd06fa829aa32bdb1bf
        9f5728118af072cb4d27b2e87c1c4abf1d744c54
      383692dec8a1036c5b62a49a9808738c5ab72075 amend by test at 1970-01-01T00:00:00 from:
        94fde643eeb6b11e10eb5de6268ce62601f8c185 split by test at 1970-01-01T00:00:00 (split into this and: 9f5728118af072cb4d27b2e87c1c4abf1d744c54) from:
          98372bb0c913529155d64663575faf5698fe8b1b
    e9a8adc18ebd9ab4986c3fb217d22ba95cefd11d histedit by test at 1970-01-01T00:00:00 from:
      cb252f4e4ec4a9befec9f4768dae810b234a03f4 histedit by test at 1970-01-01T00:00:00 from:
        47809cc234477ee2398d713e78c07c0411c569d4 histedit by test at 1970-01-01T00:00:00 from:
          d0b31d57fee70727f54b94594aec20afaa8bb34c rebase by test at 1970-01-01T00:00:00 from:
            e536de343881687fa51ea0174bd3333686cb4ced rebase by test at 1970-01-01T00:00:00 from:
              2802b58ff916d7dbca8462b9843ce7fca4ca18f4
        c0807ccf7001eeffe906fee1a5fb19223ab3740d
      7cc715a98301d3f6ae271c9b218c2c90694d005c
    6367a1362725a82dfa430133126f72113634b084 histedit by test at 1970-01-01T00:00:00 from:
      3df81c50780f689db64a5ff6ea06be268a046cf0

Histedit with exec that amends in between folds

  $ cd ..
  $ newrepo
  $ for i in 1 2 3 4
  > do
  >   echo $i >> file
  >   hg commit -Aqm "commit $i"
  > done
  $ hg histedit 0 --commands - 2>&1 <<EOF | fixbundle
  > pick c2a29f8b7d7a
  > pick 08d8367dafb9
  > fold 15a208dbcdc5
  > exec hg amend -m "commit 3 amended"
  > fold 0d4155d128bf
  > EOF
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglog
  @  8: d2088eba6321 'commit 3 amended
  |  ***
  |  commit 4'
  o  0: c2a29f8b7d7a 'commit 1'
  
  $ hg debugmutation "all()"
    c2a29f8b7d7a23d58e698384280df426802a1465
    08d8367dafb9bb90c58101707eca32b726ca635a
    15a208dbcdc54b4f841ffecf9d13f98675933242
    0d4155d128bf7fff3f12582a65b52be84ad44809
    3ad6b7b42196d53e1ed074932ed4459226226b5c histedit by test at 1970-01-01T00:00:00 from:
      15a208dbcdc54b4f841ffecf9d13f98675933242
    79c325ae812da98473285ea807b9757dc1f18eb8 histedit by test at 1970-01-01T00:00:00 from:
      08d8367dafb9bb90c58101707eca32b726ca635a
      15a208dbcdc54b4f841ffecf9d13f98675933242
    33fc94d0b2e3e9a44dd6dc39e585c88cfb3e671e amend by test at 1970-01-01T00:00:00 from:
      79c325ae812da98473285ea807b9757dc1f18eb8 histedit by test at 1970-01-01T00:00:00 from:
        08d8367dafb9bb90c58101707eca32b726ca635a
        15a208dbcdc54b4f841ffecf9d13f98675933242
    116e0130b71eee9e37a533150eb6630a42df21d2 histedit by test at 1970-01-01T00:00:00 from:
      0d4155d128bf7fff3f12582a65b52be84ad44809
    d2088eba6321240e6ab71ae17917d8db3a92abb1 histedit by test at 1970-01-01T00:00:00 from:
      33fc94d0b2e3e9a44dd6dc39e585c88cfb3e671e amend by test at 1970-01-01T00:00:00 from:
        79c325ae812da98473285ea807b9757dc1f18eb8 histedit by test at 1970-01-01T00:00:00 from:
          08d8367dafb9bb90c58101707eca32b726ca635a
          15a208dbcdc54b4f841ffecf9d13f98675933242
      0d4155d128bf7fff3f12582a65b52be84ad44809

Histedit with stop, extra commit, and fold

  $ cd ..
  $ newrepo
  $ for i in 1 2 3 4
  > do
  >   echo $i >> file
  >   hg commit -Aqm "commit $i"
  > done
  $ hg histedit 0 --commands - 2>&1 <<EOF | fixbundle
  > pick c2a29f8b7d7a
  > pick 08d8367dafb9
  > stop 15a208dbcdc5
  > fold 0d4155d128bf
  > EOF
  Changes committed as 11e6d8f98417. You may amend the changeset now.
  When you are done, run hg histedit --continue to resume
  $ echo extra >> file2
  $ hg commit -Aqm "extra commit"
  $ hg histedit --continue | fixbundle
  $ tglog
  @  7: ef799246f6e1 'extra commit
  |  ***
  |  commit 4'
  o  4: 11e6d8f98417 'commit 3'
  |
  o  1: 08d8367dafb9 'commit 2'
  |
  o  0: c2a29f8b7d7a 'commit 1'
  
  $ hg debugmutation "all()"
    c2a29f8b7d7a23d58e698384280df426802a1465
    08d8367dafb9bb90c58101707eca32b726ca635a
    15a208dbcdc54b4f841ffecf9d13f98675933242
    0d4155d128bf7fff3f12582a65b52be84ad44809
    11e6d8f98417984c3d82c3ef6c4366d3b72beb04 histedit by test at 1970-01-01T00:00:00 from:
      15a208dbcdc54b4f841ffecf9d13f98675933242
    0b78a069a4a88e74e534c80cce8e3983db06271e
    5abf9ab6d9c5268bab2aff811d4561f052f99d9e histedit by test at 1970-01-01T00:00:00 from:
      0d4155d128bf7fff3f12582a65b52be84ad44809
    ef799246f6e1caab4e24a094396f91970d71703d histedit by test at 1970-01-01T00:00:00 from:
      0b78a069a4a88e74e534c80cce8e3983db06271e
      0d4155d128bf7fff3f12582a65b52be84ad44809
