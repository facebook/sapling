#chg-compatible

TODO: configure mutation
  $ configure noevolution
  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable fbhistedit histedit rebase

  $ hg init repo
  $ cd repo
  $ echo base > base
  $ hg add base
  $ hg commit -m "base"
  $ for x in a b c ; do
  >   echo $x > $x
  >   hg add $x
  >   hg commit -m $x
  > done
  $ hg up -q 0
  $ for x in d e f ; do
  >   echo $x > $x
  >   hg add $x
  >   hg commit -m $x
  > done
  $ tglogp
  @  6: 1eb7eda15cd7 draft 'f'
  |
  o  5: 581a2eefdc84 draft 'e'
  |
  o  4: 331acda6ee00 draft 'd'
  |
  | o  3: f9d2e574dc58 draft 'c'
  | |
  | o  2: c87fe1ae405f draft 'b'
  | |
  | o  1: c604726e05fb draft 'a'
  |/
  o  0: d20a80d4def3 draft 'base'
  
Use histedit to graft an extra commit into current history

  $ hg up -q 3
  $ hg histedit 1 --commands - 2>&1 << EOF | fixbundle
  > pick c604726e05fb
  > pick c87fe1ae405f
  > graft 581a2eefdc84
  > pick f9d2e574dc58
  > EOF

  $ tglogp
  @  7: fc9a25c1b8af draft 'c'
  |
  o  6: efc3ff9af0d1 draft 'e'
  |
  | o  5: 1eb7eda15cd7 draft 'f'
  | |
  | o  4: 581a2eefdc84 draft 'e'
  | |
  | o  3: 331acda6ee00 draft 'd'
  | |
  o |  2: c87fe1ae405f draft 'b'
  | |
  o |  1: c604726e05fb draft 'a'
  |/
  o  0: d20a80d4def3 draft 'base'
  
Try to use histedit to graft a non-existent commit

  $ hg histedit 1 --commands - 2>&1 << EOF | fixbundle
  > pick c604726e05fb
  > pick c87fe1ae405f
  > graft abcdefabcdef
  > pick fc3ff9af0d1c
  > pick fc9a25c1b8af
  > EOF
  hg: parse error: unknown changeset abcdefabcdef listed

Try to use histedit to graft a commit from the set of commits being edited

  $ hg histedit 1 --commands - 2>&1 << EOF | fixbundle
  > pick c604726e05fb
  > pick c87fe1ae405f
  > graft fc9a25c1b8af
  > pick fc3ff9af0d1c
  > pick fc9a25c1b8af
  > EOF
  hg: parse error: graft "fc9a25c1b8af" changeset was an edited list candidate
  (graft must only use unlisted changesets)

