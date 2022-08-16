#chg-compatible
#debugruntest-compatible

  $ configure mutation-norecord
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
  $ hg up -q 'desc(base)'
  $ for x in d e f ; do
  >   echo $x > $x
  >   hg add $x
  >   hg commit -m $x
  > done
  $ tglogp
  @  1eb7eda15cd7 draft 'f'
  │
  o  581a2eefdc84 draft 'e'
  │
  o  331acda6ee00 draft 'd'
  │
  │ o  f9d2e574dc58 draft 'c'
  │ │
  │ o  c87fe1ae405f draft 'b'
  │ │
  │ o  c604726e05fb draft 'a'
  ├─╯
  o  d20a80d4def3 draft 'base'
  
Use histedit to graft an extra commit into current history

  $ hg up -q 'desc(c)'
  $ hg histedit 'max(desc(a))' --commands - 2>&1 << EOF | fixbundle
  > pick c604726e05fb
  > pick c87fe1ae405f
  > graft 581a2eefdc84
  > pick f9d2e574dc58
  > EOF

  $ tglogp
  @  fc9a25c1b8af draft 'c'
  │
  o  efc3ff9af0d1 draft 'e'
  │
  │ o  1eb7eda15cd7 draft 'f'
  │ │
  │ x  581a2eefdc84 draft 'e'
  │ │
  │ o  331acda6ee00 draft 'd'
  │ │
  o │  c87fe1ae405f draft 'b'
  │ │
  o │  c604726e05fb draft 'a'
  ├─╯
  o  d20a80d4def3 draft 'base'
  
Try to use histedit to graft a non-existent commit

  $ hg histedit 'max(desc(a))' --commands - 2>&1 << EOF | fixbundle
  > pick c604726e05fb
  > pick c87fe1ae405f
  > graft abcdefabcdef
  > pick fc3ff9af0d1c
  > pick fc9a25c1b8af
  > EOF
  hg: parse error: unknown changeset abcdefabcdef listed

Try to use histedit to graft a commit from the set of commits being edited

  $ hg histedit 'max(desc(a))' --commands - 2>&1 << EOF | fixbundle
  > pick c604726e05fb
  > pick c87fe1ae405f
  > graft fc9a25c1b8af
  > pick fc3ff9af0d1c
  > pick fc9a25c1b8af
  > EOF
  hg: parse error: graft "fc9a25c1b8af" changeset was an edited list candidate
  (graft must only use unlisted changesets)

