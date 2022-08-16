#chg-compatible
#debugruntest-compatible

  $ configure mutation
  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable histedit rebase
  $ setconfig phases.publish=false
  $ readconfig <<EOF
  > [ui]
  > logtemplate= {node|short} {desc|firstline}
  > EOF

Test that histedit learns about obsolescence not stored in histedit state
  $ hg init boo
  $ cd boo
  $ echo a > a
  $ hg ci -Am a
  adding a
  $ echo a > b
  $ echo a > c
  $ echo a > c
  $ hg ci -Am b
  adding b
  adding c
  $ echo a > d
  $ hg ci -Am c
  adding d
  $ echo "pick `hg log -r 'desc(a)' -T '{node|short}'`" > plan
  $ echo "pick `hg log -r 'desc(c)' -T '{node|short}'`" >> plan
  $ echo "edit `hg log -r 'desc(b)' -T '{node|short}'`" >> plan
  $ hg histedit -r 'all()' --commands plan
  Editing (1b2d564fad96), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ hg st
  A b
  A c
  ? plan
  $ hg commit --amend b
  $ hg histedit --continue
  $ hg log -G
  @  a7c0e6970599 b
  │
  o  f6ad57c4d86d c
  │
  o  cb9a9f314b8b a
  



  $ hg debugmutation -r "all()"
   *  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  
   *  f6ad57c4d86d7ab4eccce0ea15ad4ed707c7501b amend by test at 1970-01-01T00:00:00 from:
      36260dadec29b394783a666bb8103ee41ef31c29 histedit by test at 1970-01-01T00:00:00 from:
      114f4176969ef342759a8a57e6bccefc4234829b
  
   *  a7c0e697059912460046af2e00aafe7fe7ffb8db histedit by test at 1970-01-01T00:00:00 from:
      1b2d564fad96311b45362f17c2aa855150efb35f
  







With some node gone missing during the edit.

  $ echo "pick `hg log -r 'desc(a)' -T '{node|short}'`" > plan
  $ echo "pick `hg log -r 'max(desc(b))' -T '{node|short}'`" >> plan
  $ echo "edit `hg log -r 'max(desc(c))' -T '{node|short}'`" >> plan
  $ hg histedit -r 'all()' --commands plan
  Editing (f6ad57c4d86d), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ hg st
  A b
  A d
  ? plan
  $ hg commit --amend -X . -m XXXXXX
  $ hg commit --amend -X . -m b2
  $ hg --hidden debugstrip 'desc(XXXXXX)' --no-backup
  $ hg histedit --continue
  $ hg log -G
  @  54f3bb7ec5b2 c
  │
  o  12f834662cb1 b2
  │
  o  cb9a9f314b8b a
  



  $ hg debugmutation -r "all()"
   *  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  
   *  12f834662cb197db42bb7fd9d5bb517b46ccb913 amend by test at 1970-01-01T00:00:00 from:
      a9204699f34b2ac5d74b89f38d550ef653236d9b amend by test at 1970-01-01T00:00:00 from:
      9822bfbd0a4ea44a600b30cd46c5685b9ffa47f8 histedit by test at 1970-01-01T00:00:00 from:
      a7c0e697059912460046af2e00aafe7fe7ffb8db histedit by test at 1970-01-01T00:00:00 from:
      1b2d564fad96311b45362f17c2aa855150efb35f
  
   *  54f3bb7ec5b2361666b483a181f46a76dce055e0 histedit by test at 1970-01-01T00:00:00 from:
      f6ad57c4d86d7ab4eccce0ea15ad4ed707c7501b amend by test at 1970-01-01T00:00:00 from:
      36260dadec29b394783a666bb8103ee41ef31c29 histedit by test at 1970-01-01T00:00:00 from:
      114f4176969ef342759a8a57e6bccefc4234829b
  






  $ cd ..

Base setup for the rest of the testing
======================================

  $ hg init base
  $ cd base

  $ for x in a b c d e f ; do
  >     echo $x > $x
  >     hg add $x
  >     hg ci -m $x
  > done

  $ hg log --graph
  @  652413bf663e f
  │
  o  e860deea161a e
  │
  o  055a42cdd887 d
  │
  o  177f92b77385 c
  │
  o  d2ae7f538514 b
  │
  o  cb9a9f314b8b a
  




  $ HGEDITOR=cat hg histedit 'desc(b)'
  pick d2ae7f538514 b
  pick 177f92b77385 c
  pick 055a42cdd887 d
  pick e860deea161a e
  pick 652413bf663e f
  
  # Edit history between d2ae7f538514 and 652413bf663e
  #
  # Commits are listed from least to most recent
  #
  # You can reorder changesets by reordering the lines
  #
  # Commands:
  #
  #  e, edit = use commit, but stop for amending
  #  m, mess = edit commit message without changing commit content
  #  p, pick = use commit
  #  b, base = checkout changeset and apply further changesets from there
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description and date
  #



  $ hg histedit 'desc(b)' --commands - --verbose <<EOF | grep histedit
  > pick 177f92b77385 2 c
  > drop d2ae7f538514 1 b
  > pick 055a42cdd887 3 d
  > fold e860deea161a 4 e
  > pick 652413bf663e 5 f
  > EOF
  [1]
  $ hg log --graph --hidden
  @  363adb0b332c f
  │
  o  e80cad0096a5 d
  │
  │ x  c04b72554bfd d
  ├─╯
  o  dfac7d6bf3bc c
  │
  │ x  652413bf663e f
  │ │
  │ x  e860deea161a e
  │ │
  │ x  055a42cdd887 d
  │ │
  │ x  177f92b77385 c
  │ │
  │ o  d2ae7f538514 b
  ├─╯
  o  cb9a9f314b8b a
  



  $ hg debugmutation -r "all()"
   *  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  
   *  dfac7d6bf3bc0ef52e2721b05ea805990cba3627 histedit by test at 1970-01-01T00:00:00 from:
      177f92b773850b59254aa5e923436f921b55483b
  
   *  e80cad0096a561f253f8d9a465fb82ec5403f8b6 histedit by test at 1970-01-01T00:00:00 from:
      |-  c04b72554bfd2ad72e3fa8c23834904e7ab7d57c histedit by test at 1970-01-01T00:00:00 from:
      |   055a42cdd88768532f9cf79daa407fc8d138de9b
      '-  e860deea161a2f77de56603b340ebbb4536308ae
  
   *  363adb0b332ccf89563be10fb06d0c90f05fe2a8 histedit by test at 1970-01-01T00:00:00 from:
      652413bf663ef2a641cab26574e46d5f5a64a55a
  










Ensure hidden revision does not prevent histedit
-------------------------------------------------

create an hidden revision

  $ hg histedit 'max(desc(c))' --commands - << EOF
  > pick dfac7d6bf3bc 6 c
  > drop e80cad0096a5 7 d
  > pick 363adb0b332c 8 f
  > EOF
  $ hg log --graph
  @  2a7423bdcce6 f
  │
  o  dfac7d6bf3bc c
  │
  o  cb9a9f314b8b a
  

check hidden revision are ignored (6 have hidden children 7 and 8)

  $ hg histedit 'max(desc(c))' --commands - << EOF
  > pick dfac7d6bf3bc 6 c
  > pick 2a7423bdcce6 8 f
  > EOF



Test that rewriting leaving instability behind is allowed
---------------------------------------------------------------------

  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -r 'children(.)'
  2a7423bdcce6 f (no-eol)
  $ hg histedit -r '.' --commands - <<EOF
  > edit dfac7d6bf3bc 6 c
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  adding c
  Editing (dfac7d6bf3bc), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ echo c >> c
  $ hg histedit --continue

  $ hg log -r '(obsolete()::) - obsolete()'
  2a7423bdcce6 f (no-eol)

stabilise

  $ hg rebase  -r '(obsolete()::) - obsolete()' -d .
  rebasing 2a7423bdcce6 "f"
  $ hg up tip -q

Test phases support
===========================================

Check that histedit respect immutability
-------------------------------------------

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate= {node|short} ({phase}) {desc|firstline}\n
  > EOF

  $ hg debugmakepublic '.^'
  $ hg log -G
  @  c3accca457aa (draft) f
  │
  o  05d885d5bf7b (public) c
  │
  o  cb9a9f314b8b (public) a
  

  $ hg histedit -r '.~2'
  abort: cannot edit public changeset: cb9a9f314b8b
  (see 'hg help phases' for details)
  [255]


Prepare further testing
-------------------------------------------

  $ for x in g h i j k ; do
  >     echo $x > $x
  >     hg add $x
  >     hg ci -m $x
  > done
  $ hg log -G
  @  4f4f997369d1 (draft) k
  │
  o  94556afb7287 (draft) j
  │
  o  af5689bd30fd (draft) i
  │
  o  49605d76b1f7 (draft) h
  │
  o  bd6d43595d7e (draft) g
  │
  o  c3accca457aa (draft) f
  │
  o  05d885d5bf7b (public) c
  │
  o  cb9a9f314b8b (public) a
  

  $ cd ..

simple phase conservation
-------------------------------------------

Resulting changeset should conserve the phase of the original one whatever the
phases.new-commit option is.

New-commit as draft (default)

  $ cp -R base simple-draft
  $ cd simple-draft
  $ hg histedit -r 'c3accca457aa' --commands - << EOF
  > edit c3accca457aa 13 f
  > pick bd6d43595d7e 14 g
  > pick 49605d76b1f7 15 h
  > pick af5689bd30fd 16 i
  > pick 94556afb7287 17 j
  > pick 4f4f997369d1 18 k
  > EOF
  0 files updated, 0 files merged, 6 files removed, 0 files unresolved
  adding f
  Editing (c3accca457aa), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ echo f >> f
  $ hg histedit --continue
  $ hg log -G
  @  1313fc35db52 (draft) k
  │
  o  b361034ee87c (draft) j
  │
  o  c17eeb2f6c3d (draft) i
  │
  o  5af825d0adbb (draft) h
  │
  o  152765193a02 (draft) g
  │
  o  6263e4f96392 (draft) f
  │
  o  05d885d5bf7b (public) c
  │
  o  cb9a9f314b8b (public) a
  

  $ cd ..


Check abort behavior
-------------------------------------------

We checks that abort properly clean the repository so the same histedit can be
attempted later.

  $ cp -R base abort
  $ cd abort
  $ hg histedit -r 'c3accca457aa' --commands - << EOF
  > pick c3accca457aa 13 f
  > pick 49605d76b1f7 15 h
  > pick bd6d43595d7e 14 g
  > pick af5689bd30fd 16 i
  > roll 94556afb7287 17 j
  > edit 4f4f997369d1 18 k
  > EOF
  Editing (4f4f997369d1), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]

  $ hg histedit --abort
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G
  @  4f4f997369d1 (draft) k
  │
  o  94556afb7287 (draft) j
  │
  o  af5689bd30fd (draft) i
  │
  o  49605d76b1f7 (draft) h
  │
  o  bd6d43595d7e (draft) g
  │
  o  c3accca457aa (draft) f
  │
  o  05d885d5bf7b (public) c
  │
  o  cb9a9f314b8b (public) a
  


  $ hg histedit -r 'c3accca457aa' --commands - << EOF --config experimental.evolution.track-operation=1
  > pick c3accca457aa 13 f
  > pick 49605d76b1f7 15 h
  > pick bd6d43595d7e 14 g
  > pick af5689bd30fd 16 i
  > pick 94556afb7287 17 j
  > edit 4f4f997369d1 18 k
  > EOF
  Editing (4f4f997369d1), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ hg histedit --continue --config experimental.evolution.track-operation=1
  $ hg log -G
  @  dd9cf9176a2d (draft) k
  │
  o  1380d026a7bb (draft) j
  │
  o  9f336d5d47c2 (draft) i
  │
  o  55f4840bfff6 (draft) g
  │
  o  c679430403c0 (draft) h
  │
  o  c3accca457aa (draft) f
  │
  o  05d885d5bf7b (public) c
  │
  o  cb9a9f314b8b (public) a
  

  $ hg debugmutation
   *  dd9cf9176a2deef0faf6fcf8a3901f2cf9e596bb histedit by test at 1970-01-01T00:00:00 from:
      4f4f997369d15ed5b8b8400375c76d9952dbc3cd
  
