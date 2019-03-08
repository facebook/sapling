  $ enable amend rebase histedit fbhistedit

We need obsmarkers for now, to allow unstable commits
  $ enable obsstore

  $ cat >> $HGRCPATH <<EOF
  > [mutation]
  > record=true
  > enabled=true
  > date=0 0
  > [ui]
  > interactive = true
  > [templatealias]
  > sl_mutation_names = dict(amend="Amended as", rebase="Rebased to", split="Split into", fold="Folded into", histedit="Histedited to", rewrite="Rewritten into", land="Landed as")
  > sl_mutations = "{join(mutations % '({get(sl_mutation_names, operation, "Rewritten using {operation} into")} {join(successors % "{node|short}", ", ")})', ' ')}"
  > sl_mutation_descs = "{join(mutations % '({get(sl_mutation_names, operation, "Rewritten using {operation} into")} {join(successors % "{desc}", ", ")})', ' ')}"
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

Revsets

  $ hg log -T '{node}\n' -r 'predecessors(2a2702418db)' --hidden
  561937d12f41e7d2f5ade2799de1bc21b92ddc51
  1e2c46af1a22b8949201aee655b53f2aba83c490
  afdb4ea72e8cb14b34dfae49b9cc9be698468edf
  afcbdd90543ac6273d77ce2b6e967fb73373e5a4
  33905c5919f60e31c4e4f00ad5956a06848cbe10
  8ae4b2d33bbb804e1e8a5d5e43164e61dfb09885
  ded4fa782bd8c1051c8be550cebbc267572e15d0
  8462f4f357413f9f1c76a798d6ccdfc1e4337bd7
  60f9e7d031c5b05f8ff106d39a20d67c40dc7411
  2fd85d288d1b25636df6532b000fbb150e43646e
  618c9a83fb832b6742123bd06fa829aa32bdb1bf
  a7e46e8d9faf725274ea4cde6d202dd8d74991b0
  b23a10bc8972610ae489b044312b4e89e89fa08e
  98372bb0c913529155d64663575faf5698fe8b1b
  9f5728118af072cb4d27b2e87c1c4abf1d744c54
  94fde643eeb6b11e10eb5de6268ce62601f8c185
  383692dec8a1036c5b62a49a9808738c5ab72075
  f9036a3722b2b4cdbd55d08cb6cba9a38bdd01a3
  e086d79182ddf80b13bf03020e7955d523f78afc
  16c4bfbbca18238ddc7bb3946a0b6b230464799b
  2a2702418db0647c75b35bffa75ad7b4ea377e44
  $ hg log -T '{node}\n' -r 'predecessors(2a2702418db,3)' --hidden
  b23a10bc8972610ae489b044312b4e89e89fa08e
  98372bb0c913529155d64663575faf5698fe8b1b
  9f5728118af072cb4d27b2e87c1c4abf1d744c54
  94fde643eeb6b11e10eb5de6268ce62601f8c185
  383692dec8a1036c5b62a49a9808738c5ab72075
  f9036a3722b2b4cdbd55d08cb6cba9a38bdd01a3
  e086d79182ddf80b13bf03020e7955d523f78afc
  16c4bfbbca18238ddc7bb3946a0b6b230464799b
  2a2702418db0647c75b35bffa75ad7b4ea377e44
  $ hg log -T '{node}\n' -r 'predecessors(d0b31d57fee)' --hidden
  2802b58ff916d7dbca8462b9843ce7fca4ca18f4
  e536de343881687fa51ea0174bd3333686cb4ced
  d0b31d57fee70727f54b94594aec20afaa8bb34c
  $ hg log -T '{node}\n' -r 'predecessors(2802b58ff91)' --hidden
  2802b58ff916d7dbca8462b9843ce7fca4ca18f4

  $ hg log -T '{node}\n' -r 'successors(2802b58ff91)' --hidden
  2802b58ff916d7dbca8462b9843ce7fca4ca18f4
  e536de343881687fa51ea0174bd3333686cb4ced
  d0b31d57fee70727f54b94594aec20afaa8bb34c
  47809cc234477ee2398d713e78c07c0411c569d4
  cb252f4e4ec4a9befec9f4768dae810b234a03f4
  e9a8adc18ebd9ab4986c3fb217d22ba95cefd11d
  $ hg log -T '{node}\n' -r 'successors(2802b58ff91,2)' --hidden
  2802b58ff916d7dbca8462b9843ce7fca4ca18f4
  e536de343881687fa51ea0174bd3333686cb4ced
  d0b31d57fee70727f54b94594aec20afaa8bb34c
  $ hg log -T '{node}\n' -r 'successors(561937d12f4)' --hidden
  561937d12f41e7d2f5ade2799de1bc21b92ddc51
  afdb4ea72e8cb14b34dfae49b9cc9be698468edf
  33905c5919f60e31c4e4f00ad5956a06848cbe10
  ded4fa782bd8c1051c8be550cebbc267572e15d0
  2fd85d288d1b25636df6532b000fbb150e43646e
  f9036a3722b2b4cdbd55d08cb6cba9a38bdd01a3
  e086d79182ddf80b13bf03020e7955d523f78afc
  16c4bfbbca18238ddc7bb3946a0b6b230464799b
  2a2702418db0647c75b35bffa75ad7b4ea377e44
  $ hg log -T '{node}\n' -r 'successors(.)' --hidden
  6367a1362725a82dfa430133126f72113634b084

Unhide some old commits and show their mutations in the log
  $ hg unhide -q cb252f4e4ec4a9befec9f4768dae810b234a03f4
  $ hg unhide -q 618c9a83fb832b6742123bd06fa829aa32bdb1bf
  $ hg unhide -q e086d79182ddf80b13bf03020e7955d523f78afc
  $ hg unhide -q 4c8af5bba994ede28e843f607374031db8abd043
  $ hg unhide -q c5d0fa8770bdde6ef311cc640a78a2f686be28b4
  $ hg log -G -T "{node} {sl_mutations}\n" -r "all()"
  @  6367a1362725a82dfa430133126f72113634b084
  |
  o  e9a8adc18ebd9ab4986c3fb217d22ba95cefd11d
  |
  | x  cb252f4e4ec4a9befec9f4768dae810b234a03f4 (Histedited to e9a8adc18ebd)
  |/
  o  2a2702418db0647c75b35bffa75ad7b4ea377e44
  |
  | x  e086d79182ddf80b13bf03020e7955d523f78afc (Rewritten into 2a2702418db0)
  |/
  | x  618c9a83fb832b6742123bd06fa829aa32bdb1bf (Rewritten into e086d79182dd)
  | |
  | x  2fd85d288d1b25636df6532b000fbb150e43646e (Rewritten into e086d79182dd)
  |/
  o  c5fb4c2b7fcf4b995e8cd8f6b0cb5186d9b5b935
  |
  | x  4c8af5bba994ede28e843f607374031db8abd043 (Rewritten into c5fb4c2b7fcf)
  |/
  | x  c5d0fa8770bdde6ef311cc640a78a2f686be28b4 (Amended as 4c8af5bba994)
  |/
  o  d20a80d4def38df63a4b330b7fb688f3d4cae1e3
  
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

Drawdag

  $ cd ..
  $ newrepo
  $ hg debugdrawdag <<'EOS'
  >       G
  >       |
  > I D C F   # split: B -> E, F, G
  >  \ \| |   # rebase: C -> D -> H
  >   H B E   # prune: F, I
  >    \|/
  >     A
  > EOS

  $ hg log -r 'sort(all(), topo)' -G --hidden -T '{desc} {node} {sl_mutations}'
  o  I 9d3d3e8bcf0521804d5d14513461a1b43f2722ef
  |
  o  H 45d7378ca81d4ce1e9b31f0e3d567b8292dffc77
  |
  | o  G 63a5789cbb56b401dcf1c5d228d75c645df293d8
  | |
  | o  F 64a8289d249234b9886244d379f15e6b650b28e3
  | |
  | o  E 7fb047a69f220c21711122dfd94305a9efb60cba
  |/
  | x  D 78698f46e6eb5de39fc18042f71f03cb7a21285c (Rebased to 45d7378ca81d)
  | |
  | | x  C 26805aba1e600a82e93661149f2313866a221a7b (Rebased to 78698f46e6eb)
  | |/
  | x  B 112478962961147124edd43549aedd1a335e44bf (Split into 7fb047a69f22, 64a8289d2492, 63a5789cbb56)
  |/
  o  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  
  $ hg debugmutation "all()"
    426bada5c67598ca65036d57d9e4b64b0c1ce7a0
    112478962961147124edd43549aedd1a335e44bf
    26805aba1e600a82e93661149f2313866a221a7b
    7fb047a69f220c21711122dfd94305a9efb60cba
    78698f46e6eb5de39fc18042f71f03cb7a21285c rebase by test at 1970-01-01T00:00:00 from:
      26805aba1e600a82e93661149f2313866a221a7b
    64a8289d249234b9886244d379f15e6b650b28e3
    63a5789cbb56b401dcf1c5d228d75c645df293d8 split by test at 1970-01-01T00:00:00 (split into this and: 7fb047a69f220c21711122dfd94305a9efb60cba, 64a8289d249234b9886244d379f15e6b650b28e3) from:
      112478962961147124edd43549aedd1a335e44bf
    45d7378ca81d4ce1e9b31f0e3d567b8292dffc77 rebase by test at 1970-01-01T00:00:00 from:
      78698f46e6eb5de39fc18042f71f03cb7a21285c rebase by test at 1970-01-01T00:00:00 from:
        26805aba1e600a82e93661149f2313866a221a7b
    9d3d3e8bcf0521804d5d14513461a1b43f2722ef

Revsets obey visibility rules

  $ cd ..
  $ newrepo
  $ drawdag <<'EOS'
  >  E
  >  |
  >  B C D  # amend: B -> C -> D
  >   \|/   # prune: D
  >    A    # revive: C
  > EOS

  $ hg debugmutation "all()"
    426bada5c67598ca65036d57d9e4b64b0c1ce7a0
    112478962961147124edd43549aedd1a335e44bf
    948823afc5bdb8c69913d366d7220f812ecf0d41 amend by test at 1970-01-01T00:00:00 from:
      112478962961147124edd43549aedd1a335e44bf
    49cb92066bfd0763fff729c354345650b7428554
    c746b20d1d04f32a7466f736807eeef36a33b3dd amend by test at 1970-01-01T00:00:00 from:
      948823afc5bdb8c69913d366d7220f812ecf0d41 amend by test at 1970-01-01T00:00:00 from:
        112478962961147124edd43549aedd1a335e44bf
  $ hg log -T '{node} {desc}\n' -r "successors($B)"
  112478962961147124edd43549aedd1a335e44bf B
  948823afc5bdb8c69913d366d7220f812ecf0d41 C
  $ hg log -T '{node} {desc}\n' -r "successors($B)" --hidden
  112478962961147124edd43549aedd1a335e44bf B
  948823afc5bdb8c69913d366d7220f812ecf0d41 C
  c746b20d1d04f32a7466f736807eeef36a33b3dd D
  $ hg log -T '{node} {desc}\n' -r "predecessors($C)"
  112478962961147124edd43549aedd1a335e44bf B
  948823afc5bdb8c69913d366d7220f812ecf0d41 C
  $ hg hide -q $E
  $ hg log -T '{node} {desc}\n' -r "predecessors($C)"
  948823afc5bdb8c69913d366d7220f812ecf0d41 C

Revsets for filtering commits based on mutated status

  $ cd ..
  $ newrepo
  $ drawdag << EOS
  >            P
  >            |\        # amend: C -> E -> G
  >  D F     M O S       # rebase: D -> F
  >  | |     | | |
  >  C E G   L N R U     # fold: L, M -> N
  >   \|/     \| | |
  >    B       K Q T     # amend: Q -> T
  >    |        \|/      # rebase: R -> U
  >    A         A
  > EOS

  $ hg log -r "obsolete()" -T '{desc}\n'
  Q
  R
  E
  $ hg log -r "orphan()" -T '{desc}\n'
  S
  F
  P
  $ hg log -r "extinct()" -T '{desc}\n'
  $ hg log -r "obsolete()" -T '{desc}\n' --hidden
  Q
  C
  L
  R
  D
  E
  M
  $ hg log -r "orphan()" -T '{desc}\n' --hidden
  S
  F
  P
  $ hg log -r "extinct()" -T '{desc}\n' --hidden
  C
  L
  D
  M

  $ cd ..
  $ newrepo
  $ drawdag << EOS
  > E
  > |
  > D
  > |
  > C
  > |
  > B  B1  D1    # rebase: B -> B1
  > |  |   |     # rebase: D -> D1
  > A  A   A
  > EOS
  $ hg log -r "orphan()" -T '{desc}\n'
  C
  E
  $ hg log -r "extinct()" -T '{desc}\n'

Divergence and Successors Sets

  $ cd ..
  $ newrepo
  $ drawdag --print << EOS
  >  Z P         F H    # amend: A -> B -> C
  >  |/          | |    # amend: A -> D
  >  Y   A B C D E G    # split: D -> E, F
  >  |    \|/   \|/     # amend: E -> G
  >  X     X     X      # rebase: F -> H
  >                     # amend: Z -> P
  > EOS
  a3d17304151f A
  29f5c7cacb84 B
  9263b98dea84 C
  e91ad3fd8cd0 D
  8bab98b2a161 E
  fd2cd4536115 F
  b6d2081d9c92 G
  ea4b6a4451ab H
  55bd6ca2c9c5 P
  ba2b7fa7166d X
  54fe561aeb5b Y
  e67cd4473b7c Z
  $ hg phase -p $Z --hidden
  $ hg log -r "contentdivergent()" -T '{desc}\n'
  C
  G
  H
  $ hg log -r "phasedivergent()" -T '{desc}\n'
  P

  $ hg debugsuccessorssets 'all()'
  ba2b7fa7166d
      ba2b7fa7166d
  54fe561aeb5b
      54fe561aeb5b
  e67cd4473b7c
      55bd6ca2c9c5
  9263b98dea84
      9263b98dea84
  55bd6ca2c9c5
      55bd6ca2c9c5
  b6d2081d9c92
      b6d2081d9c92
  ea4b6a4451ab
      ea4b6a4451ab
  $ hg debugsuccessorssets 'all()' --hidden
  ba2b7fa7166d
      ba2b7fa7166d
  a3d17304151f
      9263b98dea84
      b6d2081d9c92 ea4b6a4451ab
  54fe561aeb5b
      54fe561aeb5b
  29f5c7cacb84
      9263b98dea84
  e91ad3fd8cd0
      b6d2081d9c92 ea4b6a4451ab
  e67cd4473b7c
      55bd6ca2c9c5
  9263b98dea84
      9263b98dea84
  8bab98b2a161
      b6d2081d9c92
  55bd6ca2c9c5
      55bd6ca2c9c5
  fd2cd4536115
      ea4b6a4451ab
  b6d2081d9c92
      b6d2081d9c92
  ea4b6a4451ab
      ea4b6a4451ab
  $ hg debugsuccessorssets 'all()' --hidden --closest
  ba2b7fa7166d
      ba2b7fa7166d
  a3d17304151f
      29f5c7cacb84
      e91ad3fd8cd0
  54fe561aeb5b
      54fe561aeb5b
  29f5c7cacb84
      9263b98dea84
  e91ad3fd8cd0
      8bab98b2a161 fd2cd4536115
  e67cd4473b7c
      55bd6ca2c9c5
  9263b98dea84
      9263b98dea84
  8bab98b2a161
      b6d2081d9c92
  55bd6ca2c9c5
      55bd6ca2c9c5
  fd2cd4536115
      ea4b6a4451ab
  b6d2081d9c92
      b6d2081d9c92
  ea4b6a4451ab
      ea4b6a4451ab

  $ cd ..
  $ newrepo
  $ drawdag --print <<'EOS'
  >                  # amend: A -> B
  >       E  G       # amend: A -> C
  >       |  |       # split: A -> D, E
  > A B C D  F H I   # fold: F, G -> H
  >  \|/  |   \|/    # amend: F -> I
  >   Z   Z    Z
  > EOS
  ac2f7407182b A
  36f08465dc73 B
  40c31c9360c7 C
  6c7c301750f1 D
  5713d5fbff8b E
  847007ced9a7 F
  e1beb503e4fb G
  996cee015648 H
  bbb2c6c770b0 I
  48b9aae0607f Z

  $ hg log -r "contentdivergent()" -T '{desc}\n'
  B
  C
  D
  I
  E
  H
  $ hg log -r "phasedivergent()" -T '{desc}\n'

  $ hg debugsuccessorssets 'all()' --hidden
  48b9aae0607f
      48b9aae0607f
  ac2f7407182b
      36f08465dc73
      40c31c9360c7
      6c7c301750f1 5713d5fbff8b
  847007ced9a7
      996cee015648
      bbb2c6c770b0
  36f08465dc73
      36f08465dc73
  40c31c9360c7
      40c31c9360c7
  6c7c301750f1
      6c7c301750f1
  e1beb503e4fb
      996cee015648
  bbb2c6c770b0
      bbb2c6c770b0
  5713d5fbff8b
      5713d5fbff8b
  996cee015648
      996cee015648
  $ hg debugsuccessorssets 'all()' --closest
  48b9aae0607f
      48b9aae0607f
  36f08465dc73
      36f08465dc73
  40c31c9360c7
      40c31c9360c7
  6c7c301750f1
      6c7c301750f1
  bbb2c6c770b0
      bbb2c6c770b0
  5713d5fbff8b
      5713d5fbff8b
  996cee015648
      996cee015648
  $ hg debugsuccessorssets 'all()' --closest --hidden
  48b9aae0607f
      48b9aae0607f
  ac2f7407182b
      36f08465dc73
      40c31c9360c7
      6c7c301750f1 5713d5fbff8b
  847007ced9a7
      996cee015648
      bbb2c6c770b0
  36f08465dc73
      36f08465dc73
  40c31c9360c7
      40c31c9360c7
  6c7c301750f1
      6c7c301750f1
  e1beb503e4fb
      996cee015648
  bbb2c6c770b0
      bbb2c6c770b0
  5713d5fbff8b
      5713d5fbff8b
  996cee015648
      996cee015648

  $ cd ..
  $ newrepo
  $ drawdag --print <<'EOS'
  >                         # amend: A -> B
  >       E K       G   O   # amend: A -> C
  >       | |       |   |   # split: A -> D, E
  > A B C D J L M N F H I   # fold: F, G -> H
  >  \|/   \|/   \|  \|/    # amend: F -> I
  >   Z     Z     Z   Z     # amend: D -> J
  >                         # rebase: E -> K
  >                         # fold: J, K -> L
  >                         # amend: B -> M -> N
  >                         # rebase: C -> O
  > EOS
  ac2f7407182b A
  36f08465dc73 B
  40c31c9360c7 C
  6c7c301750f1 D
  5713d5fbff8b E
  847007ced9a7 F
  e1beb503e4fb G
  996cee015648 H
  bbb2c6c770b0 I
  4aebccbe0e17 J
  846a43ee5a88 K
  2ee9ab4882bd L
  84dbc8e53ce0 M
  74065e09cc11 N
  f019b41d3b77 O
  48b9aae0607f Z

  $ hg log -r "contentdivergent()" -T '{desc}\n'
  I
  H
  O
  N
  L

  $ hg debugsuccessorssets 'all()'
  48b9aae0607f
      48b9aae0607f
  bbb2c6c770b0
      bbb2c6c770b0
  996cee015648
      996cee015648
  f019b41d3b77
      f019b41d3b77
  74065e09cc11
      74065e09cc11
  2ee9ab4882bd
      2ee9ab4882bd
  $ hg debugsuccessorssets 'all()' --hidden
  48b9aae0607f
      48b9aae0607f
  ac2f7407182b
      2ee9ab4882bd
      74065e09cc11
      f019b41d3b77
  847007ced9a7
      996cee015648
      bbb2c6c770b0
  36f08465dc73
      74065e09cc11
  40c31c9360c7
      f019b41d3b77
  6c7c301750f1
      2ee9ab4882bd
  e1beb503e4fb
      996cee015648
  bbb2c6c770b0
      bbb2c6c770b0
  5713d5fbff8b
      2ee9ab4882bd
  996cee015648
      996cee015648
  4aebccbe0e17
      2ee9ab4882bd
  84dbc8e53ce0
      74065e09cc11
  f019b41d3b77
      f019b41d3b77
  846a43ee5a88
      2ee9ab4882bd
  74065e09cc11
      74065e09cc11
  2ee9ab4882bd
      2ee9ab4882bd
  $ hg debugsuccessorssets 'all()' --closest
  48b9aae0607f
      48b9aae0607f
  bbb2c6c770b0
      bbb2c6c770b0
  996cee015648
      996cee015648
  f019b41d3b77
      f019b41d3b77
  74065e09cc11
      74065e09cc11
  2ee9ab4882bd
      2ee9ab4882bd
  $ hg debugsuccessorssets 'all()' --closest --hidden
  48b9aae0607f
      48b9aae0607f
  ac2f7407182b
      36f08465dc73
      40c31c9360c7
      6c7c301750f1 5713d5fbff8b
  847007ced9a7
      996cee015648
      bbb2c6c770b0
  36f08465dc73
      84dbc8e53ce0
  40c31c9360c7
      f019b41d3b77
  6c7c301750f1
      4aebccbe0e17
  e1beb503e4fb
      996cee015648
  bbb2c6c770b0
      bbb2c6c770b0
  5713d5fbff8b
      846a43ee5a88
  996cee015648
      996cee015648
  4aebccbe0e17
      2ee9ab4882bd
  84dbc8e53ce0
      74065e09cc11
  f019b41d3b77
      f019b41d3b77
  846a43ee5a88
      2ee9ab4882bd
  74065e09cc11
      74065e09cc11
  2ee9ab4882bd
      2ee9ab4882bd

Many splits and folds:

  $ cd ..
  $ newrepo
  $ drawdag --print <<'EOS'
  >     G    R   P           # split: A -> B, C
  >     |    |   |           # split: B -> D, E
  >     F  J Q N O           # split: C -> F, G
  >     |  |/  |/            # split: A -> H, I, J
  >   C E  I L M             # fold: H, I -> K
  >   | |  | |/              # rebase: J -> L
  > A B D  H K               # split: L -> M, N
  >  \|/    \|               # split: N -> O, P
  >   Z      Z               # split: J -> Q, R
  > EOS
  ac2f7407182b A
  f0a671a46792 B
  5c43be2708c4 C
  6c7c301750f1 D
  cabf2db621a7 E
  d2478cddeb08 F
  a6215fa93efa G
  45724aa2168b H
  34d53c2267d8 I
  2bcf00cdb39b J
  207ff838d27e K
  813b6aa1d16f L
  029c898a0df7 M
  448ce1426237 N
  396902f0a26c O
  1c4d29f9f72d P
  444227ba9301 Q
  ff341471414e R
  48b9aae0607f Z

  $ hg log -r "contentdivergent()" -T '{desc}\n'
  H
  D
  I
  E
  K
  F
  Q
  G
  M
  R
  O
  P
  $ hg log -r "contentdivergent()" -T '{desc}\n' --hidden
  B
  H
  C
  D
  I
  E
  K
  F
  L
  Q
  G
  M
  R
  N
  O
  P

  $ hg debugsuccessorssets $A --hidden
  ac2f7407182b
      207ff838d27e 029c898a0df7 396902f0a26c 1c4d29f9f72d
      207ff838d27e 444227ba9301 ff341471414e
      6c7c301750f1 cabf2db621a7 d2478cddeb08 a6215fa93efa
  $ hg debugsuccessorssets $A --closest --hidden
  ac2f7407182b
      45724aa2168b 34d53c2267d8 2bcf00cdb39b
      f0a671a46792 5c43be2708c4
  $ hg unhide $A $L
  $ hg debugsuccessorssets $A
  ac2f7407182b
      207ff838d27e 029c898a0df7 396902f0a26c 1c4d29f9f72d
      207ff838d27e 444227ba9301 ff341471414e
      6c7c301750f1 cabf2db621a7 d2478cddeb08 a6215fa93efa
  $ hg debugsuccessorssets $A --closest
  ac2f7407182b
      45724aa2168b 34d53c2267d8 444227ba9301 ff341471414e
      45724aa2168b 34d53c2267d8 813b6aa1d16f
      6c7c301750f1 cabf2db621a7 d2478cddeb08 a6215fa93efa
  $ hg hide $P
  hiding commit 1c4d29f9f72d "P"
  1 changesets hidden
  $ hg debugsuccessorssets $A
  ac2f7407182b
      207ff838d27e 029c898a0df7 396902f0a26c
      207ff838d27e 444227ba9301 ff341471414e
      6c7c301750f1 cabf2db621a7 d2478cddeb08 a6215fa93efa
  $ hg log -r "all()" -T "{desc} {sl_mutation_descs}\n"
  Z 
  A (Split into H, I, Q, R) (Split into H, I, L) (Split into D, E, F, G)
  H (Folded into K)
  D 
  I (Folded into K)
  E 
  K 
  F 
  L (Split into M, O)
  Q 
  G 
  M 
  R 
  O 

Rebase of amended commit

  $ cd ..
  $ newrepo
  $ drawdag << 'EOS'
  > D     E
  > |     |
  > C  C1 C2 C3 C4 # amend: C -> C1 -> C2 -> C3 -> C4
  >  \ | /    \ /
  >   \|/      B
  >    B
  >    |
  >    A A1
  >    |/
  >    Z
  > EOS
  $ hg rebase -s $B -d $A1
  rebasing 3:917a077edb8d "B"
  rebasing 4:b45c90359798 "C"
  rebasing 6:01f26f1a10b2 "D"
  rebasing 7:93a74dbee093 "C2"
  rebasing 9:516f559bdadf "E"
  rebasing 10:c0c251cf4c45 "C4" (tip)
  $ hg log -G -T "{desc} {sl_mutation_descs}\n" -r "all()"
  o  C4
  |
  | o  E
  | |
  | x  C2 (Rewritten using rebase-copy into C4)
  |/
  | o  D
  | |
  | x  C (Rewritten using rebase-copy into C2)
  |/
  o  B
  |
  o  A1
  |
  | o  A
  |/
  o  Z
  
Rebase of folded commit

  $ cd ..
  $ newrepo
  $ drawdag << 'EOS'
  > D E
  >  \|      # fold: C, E -> F
  >   C F
  >   |/
  >   B
  >   |
  >   A A1
  >   |/
  >   Z
  > EOS
  $ hg log -G -T "{desc} {sl_mutation_descs}\n" -r "all()"
  o  F
  |
  | o  D
  | |
  | x  C (Folded into F)
  |/
  o  B
  |
  | o  A1
  | |
  o |  A
  |/
  o  Z
  
  $ hg rebase -s $B -d $A1
  rebasing 3:917a077edb8d "B"
  rebasing 4:b45c90359798 "C"
  rebasing 5:01f26f1a10b2 "D"
  rebasing 7:9eb924bd504e "F" (tip)
  $ hg log -G -T "{desc} {sl_mutation_descs}\n" -r "all()"
  o  F
  |
  | o  D
  | |
  | x  C (Rewritten using rebase-copy into F)
  |/
  o  B
  |
  o  A1
  |
  | o  A
  |/
  o  Z
  

Metaedit automatic rebase of amended commit

  $ cd ..
  $ newrepo
  $ drawdag << 'EOS'
  > D
  > |
  > C  C1 C2  # amend: C -> C1 -> C2
  >  \ | /
  >   \|/
  >    B
  >    |
  >    A
  > EOS
  $ hg metaedit -r $B -m B1
  $ hg log -G -T "{desc} {sl_mutation_descs}\n" -r "all()"
  o  C2
  |
  | o  D
  | |
  | x  C (Rewritten using metaedit-copy into C2)
  |/
  o  B1
  |
  o  A
  
Landing

  $ cd ..
  $ newrepo
  $ drawdag << EOS
  > X         # amend: A -> B -> C
  > |         # rebase: C -> X
  > Y  A B C
  > |   \|/
  > Z    Z
  > EOS
  $ hg unhide $B
  $ hg log -G -r "all()" -T "{desc} {sl_mutation_descs}\n"
  o  X
  |
  | x  B (Rewritten into X)
  | |
  o |  Y
  |/
  o  Z
  
Because we don't have a successor index for the changelog, and we only build the cache
for draft commits, we lose the fact that B was landed.

  $ hg phase -p $X
  $ hg log -G -r "all()" -T "{desc} {sl_mutation_descs}\n"
  o  X
  |
  | o  B
  | |
  o |  Y
  |/
  o  Z
  
We can restore it by re-introducing the links via mutation records.  This is a temporary
hack until we write an indexed changelog that lets us do the successor lookup for any
commit cheaply.  Normally the pullcreatemarkers and pushrebase extensions will do this
for us, but for this test we do it manually.

  $ hg debugsh --hidden -c "from edenscm.mercurial import mutation; mutation.recordentries(repo, [mutation.createsyntheticentry(repo, mutation.ORIGIN_SYNTHETIC, [repo[\"$B\"].node()], repo[\"$C\"].node(), \"land\")], skipexisting=False)"
  $ hg debugsh --hidden -c "from edenscm.mercurial import mutation; mutation.recordentries(repo, [mutation.createsyntheticentry(repo, mutation.ORIGIN_SYNTHETIC, [repo[\"$C\"].node()], repo[\"$X\"].node(), \"land\")], skipexisting=False)"
  $ hg log -G -r "all()" -T "{desc} {sl_mutation_descs}\n"
  o  X
  |
  | x  B (Landed as X)
  | |
  o |  Y
  |/
  o  Z
  
