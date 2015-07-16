  $ . "$TESTDIR/histedit-helpers.sh"

Enable obsolete

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate= {rev}:{node|short} {desc|firstline}
  > [phases]
  > publish=False
  > [experimental]
  > evolution=createmarkers,allowunstable
  > [extensions]
  > histedit=
  > rebase=
  > EOF

  $ hg init base
  $ cd base

  $ for x in a b c d e f ; do
  >     echo $x > $x
  >     hg add $x
  >     hg ci -m $x
  > done

  $ hg log --graph
  @  5:652413bf663e f
  |
  o  4:e860deea161a e
  |
  o  3:055a42cdd887 d
  |
  o  2:177f92b77385 c
  |
  o  1:d2ae7f538514 b
  |
  o  0:cb9a9f314b8b a
  

  $ HGEDITOR=cat hg histedit 1
  pick d2ae7f538514 1 b
  pick 177f92b77385 2 c
  pick 055a42cdd887 3 d
  pick e860deea161a 4 e
  pick 652413bf663e 5 f
  
  # Edit history between d2ae7f538514 and 652413bf663e
  #
  # Commits are listed from least to most recent
  #
  # Commands:
  #  p, pick = use commit
  #  e, edit = use commit, but stop for amending
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description
  #  d, drop = remove commit from history
  #  m, mess = edit message without changing commit content
  #
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg histedit 1 --commands - --verbose <<EOF | grep histedit
  > pick 177f92b77385 2 c
  > drop d2ae7f538514 1 b
  > pick 055a42cdd887 3 d
  > fold e860deea161a 4 e
  > pick 652413bf663e 5 f
  > EOF
  [1]
  $ hg log --graph --hidden
  @  10:cacdfd884a93 f
  |
  o  9:59d9f330561f d
  |
  | x  8:b558abc46d09 fold-temp-revision e860deea161a
  | |
  | x  7:96e494a2d553 d
  |/
  o  6:b346ab9a313d c
  |
  | x  5:652413bf663e f
  | |
  | x  4:e860deea161a e
  | |
  | x  3:055a42cdd887 d
  | |
  | x  2:177f92b77385 c
  | |
  | x  1:d2ae7f538514 b
  |/
  o  0:cb9a9f314b8b a
  
  $ hg debugobsolete
  d2ae7f538514cd87c17547b0de4cea71fe1af9fb 0 {cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b} (*) {'user': 'test'} (glob)
  177f92b773850b59254aa5e923436f921b55483b b346ab9a313db8537ecf96fca3ca3ca984ef3bd7 0 (*) {'user': 'test'} (glob)
  055a42cdd88768532f9cf79daa407fc8d138de9b 59d9f330561fd6c88b1a6b32f0e45034d88db784 0 (*) {'user': 'test'} (glob)
  e860deea161a2f77de56603b340ebbb4536308ae 59d9f330561fd6c88b1a6b32f0e45034d88db784 0 (*) {'user': 'test'} (glob)
  652413bf663ef2a641cab26574e46d5f5a64a55a cacdfd884a9321ec4e1de275ef3949fa953a1f83 0 (*) {'user': 'test'} (glob)
  96e494a2d553dd05902ba1cee1d94d4cb7b8faed 0 {b346ab9a313db8537ecf96fca3ca3ca984ef3bd7} (*) {'user': 'test'} (glob)
  b558abc46d09c30f57ac31e85a8a3d64d2e906e4 0 {96e494a2d553dd05902ba1cee1d94d4cb7b8faed} (*) {'user': 'test'} (glob)


Ensure hidden revision does not prevent histedit
-------------------------------------------------

create an hidden revision

  $ hg histedit 6 --commands - << EOF
  > pick b346ab9a313d 6 c
  > drop 59d9f330561f 7 d
  > pick cacdfd884a93 8 f
  > EOF
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log --graph
  @  11:c13eb81022ca f
  |
  o  6:b346ab9a313d c
  |
  o  0:cb9a9f314b8b a
  
check hidden revision are ignored (6 have hidden children 7 and 8)

  $ hg histedit 6 --commands - << EOF
  > pick b346ab9a313d 6 c
  > pick c13eb81022ca 8 f
  > EOF
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved



Test that rewriting leaving instability behind is allowed
---------------------------------------------------------------------

  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -r 'children(.)'
  11:c13eb81022ca f (no-eol)
  $ hg histedit -r '.' --commands - <<EOF
  > edit b346ab9a313d 6 c
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  adding c
  Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.
  [1]
  $ echo c >> c
  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -r 'unstable()'
  11:c13eb81022ca f (no-eol)

stabilise

  $ hg rebase  -r 'unstable()' -d .
  rebasing 11:c13eb81022ca "f"
  $ hg up tip -q

Test dropping of changeset on the top of the stack
-------------------------------------------------------

Nothing is rewritten below, the working directory parent must be change for the
dropped changeset to be hidden.

  $ cd ..
  $ hg clone base droplast
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd droplast
  $ hg histedit -r '40db8afa467b' --commands - << EOF
  > pick 40db8afa467b 10 c
  > drop b449568bf7fc 11 f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -G
  @  12:40db8afa467b c
  |
  o  0:cb9a9f314b8b a
  

With rewritten ancestors

  $ echo e > e
  $ hg add e
  $ hg commit -m g
  $ echo f > f
  $ hg add f
  $ hg commit -m h
  $ hg histedit -r '40db8afa467b' --commands - << EOF
  > pick 47a8561c0449 12 g
  > pick 40db8afa467b 10 c
  > drop 1b3b05f35ff0 13 h
  > EOF
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G
  @  17:ee6544123ab8 c
  |
  o  16:269e713e9eae g
  |
  o  0:cb9a9f314b8b a
  
  $ cd ../base



Test phases support
===========================================

Check that histedit respect immutability
-------------------------------------------

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate= {rev}:{node|short} ({phase}) {desc|firstline}\n
  > EOF

  $ hg ph -pv '.^'
  phase changed for 2 changesets
  $ hg log -G
  @  13:b449568bf7fc (draft) f
  |
  o  12:40db8afa467b (public) c
  |
  o  0:cb9a9f314b8b (public) a
  
  $ hg histedit -r '.~2'
  abort: cannot edit public changeset: cb9a9f314b8b
  (see "hg help phases" for details)
  [255]


Prepare further testing
-------------------------------------------

  $ for x in g h i j k ; do
  >     echo $x > $x
  >     hg add $x
  >     hg ci -m $x
  > done
  $ hg phase --force --secret .~2
  $ hg log -G
  @  18:ee118ab9fa44 (secret) k
  |
  o  17:3a6c53ee7f3d (secret) j
  |
  o  16:b605fb7503f2 (secret) i
  |
  o  15:7395e1ff83bd (draft) h
  |
  o  14:6b70183d2492 (draft) g
  |
  o  13:b449568bf7fc (draft) f
  |
  o  12:40db8afa467b (public) c
  |
  o  0:cb9a9f314b8b (public) a
  
  $ cd ..

simple phase conservation
-------------------------------------------

Resulting changeset should conserve the phase of the original one whatever the
phases.new-commit option is.

New-commit as draft (default)

  $ cp -r base simple-draft
  $ cd simple-draft
  $ hg histedit -r 'b449568bf7fc' --commands - << EOF
  > edit b449568bf7fc 11 f
  > pick 6b70183d2492 12 g
  > pick 7395e1ff83bd 13 h
  > pick b605fb7503f2 14 i
  > pick 3a6c53ee7f3d 15 j
  > pick ee118ab9fa44 16 k
  > EOF
  0 files updated, 0 files merged, 6 files removed, 0 files unresolved
  adding f
  Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.
  [1]
  $ echo f >> f
  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G
  @  24:12e89af74238 (secret) k
  |
  o  23:636a8687b22e (secret) j
  |
  o  22:ccaf0a38653f (secret) i
  |
  o  21:11a89d1c2613 (draft) h
  |
  o  20:c1dec7ca82ea (draft) g
  |
  o  19:087281e68428 (draft) f
  |
  o  12:40db8afa467b (public) c
  |
  o  0:cb9a9f314b8b (public) a
  
  $ cd ..


New-commit as draft (default)

  $ cp -r base simple-secret
  $ cd simple-secret
  $ cat >> .hg/hgrc << EOF
  > [phases]
  > new-commit=secret
  > EOF
  $ hg histedit -r 'b449568bf7fc' --commands - << EOF
  > edit b449568bf7fc 11 f
  > pick 6b70183d2492 12 g
  > pick 7395e1ff83bd 13 h
  > pick b605fb7503f2 14 i
  > pick 3a6c53ee7f3d 15 j
  > pick ee118ab9fa44 16 k
  > EOF
  0 files updated, 0 files merged, 6 files removed, 0 files unresolved
  adding f
  Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.
  [1]
  $ echo f >> f
  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G
  @  24:12e89af74238 (secret) k
  |
  o  23:636a8687b22e (secret) j
  |
  o  22:ccaf0a38653f (secret) i
  |
  o  21:11a89d1c2613 (draft) h
  |
  o  20:c1dec7ca82ea (draft) g
  |
  o  19:087281e68428 (draft) f
  |
  o  12:40db8afa467b (public) c
  |
  o  0:cb9a9f314b8b (public) a
  
  $ cd ..


Changeset reordering
-------------------------------------------

If a secret changeset is put before a draft one, all descendant should be secret.
It seems more important to present the secret phase.

  $ cp -r base reorder
  $ cd reorder
  $ hg histedit -r 'b449568bf7fc' --commands - << EOF
  > pick b449568bf7fc 11 f
  > pick 3a6c53ee7f3d 15 j
  > pick 6b70183d2492 12 g
  > pick b605fb7503f2 14 i
  > pick 7395e1ff83bd 13 h
  > pick ee118ab9fa44 16 k
  > EOF
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G
  @  23:558246857888 (secret) k
  |
  o  22:28bd44768535 (secret) h
  |
  o  21:d5395202aeb9 (secret) i
  |
  o  20:21edda8e341b (secret) g
  |
  o  19:5ab64f3a4832 (secret) j
  |
  o  13:b449568bf7fc (draft) f
  |
  o  12:40db8afa467b (public) c
  |
  o  0:cb9a9f314b8b (public) a
  
  $ cd ..

Changeset folding
-------------------------------------------

Folding a secret changeset with a draft one turn the result secret (again,
better safe than sorry). Folding between same phase changeset still works

Note that there is a few reordering in this series for more extensive test

  $ cp -r base folding
  $ cd folding
  $ cat >> .hg/hgrc << EOF
  > [phases]
  > new-commit=secret
  > EOF
  $ hg histedit -r 'b449568bf7fc' --commands - << EOF
  > pick 7395e1ff83bd 13 h
  > fold b449568bf7fc 11 f
  > pick 6b70183d2492 12 g
  > fold 3a6c53ee7f3d 15 j
  > pick b605fb7503f2 14 i
  > fold ee118ab9fa44 16 k
  > EOF
  0 files updated, 0 files merged, 6 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G
  @  27:f9daec13fb98 (secret) i
  |
  o  24:49807617f46a (secret) g
  |
  o  21:050280826e04 (draft) h
  |
  o  12:40db8afa467b (public) c
  |
  o  0:cb9a9f314b8b (public) a
  
  $ hg co 24
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo wat >> wat
  $ hg add wat
  $ hg ci -m 'add wat'
  created new head
  $ hg merge 27
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge'
  $ echo not wat > wat
  $ hg ci -m 'modify wat'
  $ hg histedit 21
  abort: cannot edit history that contains merges
  [255]
  $ cd ..
