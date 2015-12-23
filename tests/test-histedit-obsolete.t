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
  #
  #  e, edit = use commit, but stop for amending
  #  m, mess = edit commit message without changing commit content
  #  p, pick = use commit
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description
  #
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
  96e494a2d553dd05902ba1cee1d94d4cb7b8faed 0 {b346ab9a313db8537ecf96fca3ca3ca984ef3bd7} (*) {'user': 'test'} (glob)
  b558abc46d09c30f57ac31e85a8a3d64d2e906e4 0 {96e494a2d553dd05902ba1cee1d94d4cb7b8faed} (*) {'user': 'test'} (glob)
  d2ae7f538514cd87c17547b0de4cea71fe1af9fb 0 {cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b} (*) {'user': 'test'} (glob)
  177f92b773850b59254aa5e923436f921b55483b b346ab9a313db8537ecf96fca3ca3ca984ef3bd7 0 (*) {'user': 'test'} (glob)
  055a42cdd88768532f9cf79daa407fc8d138de9b 59d9f330561fd6c88b1a6b32f0e45034d88db784 0 (*) {'user': 'test'} (glob)
  e860deea161a2f77de56603b340ebbb4536308ae 59d9f330561fd6c88b1a6b32f0e45034d88db784 0 (*) {'user': 'test'} (glob)
  652413bf663ef2a641cab26574e46d5f5a64a55a cacdfd884a9321ec4e1de275ef3949fa953a1f83 0 (*) {'user': 'test'} (glob)


Ensure hidden revision does not prevent histedit
-------------------------------------------------

create an hidden revision

  $ hg histedit 6 --commands - << EOF
  > pick b346ab9a313d 6 c
  > drop 59d9f330561f 7 d
  > pick cacdfd884a93 8 f
  > EOF
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
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
  Editing (b346ab9a313d), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ echo c >> c
  $ hg histedit --continue

  $ hg log -r 'unstable()'
  11:c13eb81022ca f (no-eol)

stabilise

  $ hg rebase  -r 'unstable()' -d .
  rebasing 11:c13eb81022ca "f"
  $ hg up tip -q

check that extra has accumulated from histedit and rebase

  $ hg log -T '{extras % "{key}={value}\n"}\n' -r tip
  branch=default
  histedit_source=cacdfd884a9321ec4e1de275ef3949fa953a1f83
  rebase_source=c13eb81022caa686a369223fe7f926bc4f7db576
  

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
  > drop 947ece25170f 11 f
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
  @  13:947ece25170f (draft) f
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
  @  18:14bda137d5b3 (secret) k
  |
  o  17:c62e7241a4f2 (secret) j
  |
  o  16:9cd3934e05af (secret) i
  |
  o  15:ee4a24fc4dfa (draft) h
  |
  o  14:d22905de3528 (draft) g
  |
  o  13:947ece25170f (draft) f
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
  $ hg histedit -r '947ece25170f' --commands - << EOF
  > edit 947ece25170f 11 f
  > pick d22905de3528 12 g
  > pick ee4a24fc4dfa 13 h
  > pick 9cd3934e05af 14 i
  > pick c62e7241a4f2 15 j
  > pick 14bda137d5b3 16 k
  > EOF
  0 files updated, 0 files merged, 6 files removed, 0 files unresolved
  adding f
  Editing (947ece25170f), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ echo f >> f
  $ hg histedit --continue
  $ hg log -G
  @  24:12925f763c90 (secret) k
  |
  o  23:4545a6e77442 (secret) j
  |
  o  22:d947a0798e76 (secret) i
  |
  o  21:28fb35ae4ebb (draft) h
  |
  o  20:10b22a5a9645 (draft) g
  |
  o  19:c5a1db4a69f5 (draft) f
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
  $ hg histedit -r '947ece25170f' --commands - << EOF
  > edit 947ece25170f 11 f
  > pick d22905de3528 12 g
  > pick ee4a24fc4dfa 13 h
  > pick 9cd3934e05af 14 i
  > pick c62e7241a4f2 15 j
  > pick 14bda137d5b3 16 k
  > EOF
  0 files updated, 0 files merged, 6 files removed, 0 files unresolved
  adding f
  Editing (947ece25170f), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ echo f >> f
  $ hg histedit --continue
  $ hg log -G
  @  24:12925f763c90 (secret) k
  |
  o  23:4545a6e77442 (secret) j
  |
  o  22:d947a0798e76 (secret) i
  |
  o  21:28fb35ae4ebb (draft) h
  |
  o  20:10b22a5a9645 (draft) g
  |
  o  19:c5a1db4a69f5 (draft) f
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
  $ hg histedit -r '947ece25170f' --commands - << EOF
  > pick 947ece25170f 11 f
  > pick c62e7241a4f2 15 j
  > pick d22905de3528 12 g
  > pick 9cd3934e05af 14 i
  > pick ee4a24fc4dfa 13 h
  > pick 14bda137d5b3 16 k
  > EOF
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved
  $ hg log -G
  @  23:9e712162b2c1 (secret) k
  |
  o  22:490861543602 (secret) h
  |
  o  21:86aeda50b70d (secret) i
  |
  o  20:b2fa360bc090 (secret) g
  |
  o  19:e10fb4e3eb8e (secret) j
  |
  o  13:947ece25170f (draft) f
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
  $ hg histedit -r '947ece25170f' --commands - << EOF
  > pick ee4a24fc4dfa 13 h
  > fold 947ece25170f 11 f
  > pick d22905de3528 12 g
  > fold c62e7241a4f2 15 j
  > pick 9cd3934e05af 14 i
  > fold 14bda137d5b3 16 k
  > EOF
  0 files updated, 0 files merged, 6 files removed, 0 files unresolved
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G
  @  27:769e8ee8708e (secret) i
  |
  o  24:3de6dbab1b62 (secret) g
  |
  o  21:1d51647632b2 (draft) h
  |
  o  12:40db8afa467b (public) c
  |
  o  0:cb9a9f314b8b (public) a
  
  $ hg co 3de6dbab1b62
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo wat >> wat
  $ hg add wat
  $ hg ci -m 'add wat'
  created new head
  $ hg merge 769e8ee8708e
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge'
  $ echo not wat > wat
  $ hg ci -m 'modify wat'
  $ hg histedit 1d51647632b2
  abort: cannot edit history that contains merges
  [255]
  $ cd ..
