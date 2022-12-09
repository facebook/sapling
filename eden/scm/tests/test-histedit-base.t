#chg-compatible
#debugruntest-compatible

  $ setconfig workingcopy.ruststatus=False
  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable histedit

Create repo a:

  $ hg init a
  $ cd a
  $ setconfig extensions.treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py
  $ setconfig treemanifest.server=True ui.allowemptycommit=True
  $ hg commit -qm "A"
  $ hg commit -qm "B"
  $ hg commit -qm "C"
  $ hg commit -qm "D"
  $ hg up -q .~3
  $ hg commit -qm "E"
  $ hg book E
  $ hg up -q .~1
  $ hg commit -qm "F"
  $ hg merge -q E
  $ hg book -d E
  $ hg commit -qm "G"
  $ hg up -q .^
  $ hg commit -qm "H"

  $ tglogp
  @  23a00112b28c draft 'H'
  │
  │ o  319f51d6224e draft 'G'
  ╭─┤
  o │  971baba67099 draft 'F'
  │ │
  │ o  0e89a44ca1b2 draft 'E'
  ├─╯
  │ o  9da08f1f4bcc draft 'D'
  │ │
  │ o  9b96ea441fce draft 'C'
  │ │
  │ o  f68855660cff draft 'B'
  ├─╯
  o  7b3f3d5e5faf draft 'A'
  
Verify that implicit base command and help are listed

  $ HGEDITOR=cat hg histedit |grep base
  #  b, base = checkout changeset and apply further changesets from there

Go to D
  $ hg goto 'desc(D)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
edit the history to rebase B onto H


Rebase B onto H
  $ hg histedit 'max(desc(B))' --commands - 2>&1 << EOF | fixbundle
  > base 23a00112b28c 
  > pick f68855660cff B
  > pick 9b96ea441fce C
  > pick 9da08f1f4bcc D
  > EOF

  $ tglogp
  @  8e332b0db783 draft 'D'
  │
  o  fb0676d5bfd4 draft 'C'
  │
  o  047a902d2bc7 draft 'B'
  │
  o  23a00112b28c draft 'H'
  │
  │ o  319f51d6224e draft 'G'
  ╭─┤
  o │  971baba67099 draft 'F'
  │ │
  │ o  0e89a44ca1b2 draft 'E'
  ├─╯
  o  7b3f3d5e5faf draft 'A'
  
Rebase back and drop something
  $ hg histedit 'max(desc(B))' --commands - 2>&1 << EOF | fixbundle
  > base 7b3f3d5e5faf
  > pick 047a902d2bc7 B
  > drop fb0676d5bfd4 C
  > pick 8e332b0db783 D
  > EOF

  $ tglogp
  @  22b78c3c2883 draft 'D'
  │
  o  cd1f16922537 draft 'B'
  │
  │ o  23a00112b28c draft 'H'
  │ │
  │ │ o  319f51d6224e draft 'G'
  │ ╭─┤
  │ o │  971baba67099 draft 'F'
  ├─╯ │
  │   o  0e89a44ca1b2 draft 'E'
  ├───╯
  o  7b3f3d5e5faf draft 'A'
  
Split stack
  $ hg histedit 'max(desc(B))' --commands - 2>&1 << EOF | fixbundle
  > base 7b3f3d5e5faf
  > pick cd1f16922537 B
  > base 7b3f3d5e5faf C
  > pick 22b78c3c2883 D
  > EOF

  $ tglogp
  @  3849e69e0651 draft 'D'
  │
  │ o  cd1f16922537 draft 'B'
  ├─╯
  │ o  23a00112b28c draft 'H'
  │ │
  │ │ o  319f51d6224e draft 'G'
  │ ╭─┤
  │ o │  971baba67099 draft 'F'
  ├─╯ │
  │   o  0e89a44ca1b2 draft 'E'
  ├───╯
  o  7b3f3d5e5faf draft 'A'
  
Abort
  $ echo x > B
  $ hg add B
  $ hg commit -m "X"
  $ tglogp
  @  5d4ea538b61e draft 'X'
  │
  o  3849e69e0651 draft 'D'
  │
  │ o  cd1f16922537 draft 'B'
  ├─╯
  │ o  23a00112b28c draft 'H'
  │ │
  │ │ o  319f51d6224e draft 'G'
  │ ╭─┤
  │ o │  971baba67099 draft 'F'
  ├─╯ │
  │   o  0e89a44ca1b2 draft 'E'
  ├───╯
  o  7b3f3d5e5faf draft 'A'
  
Continue
  $ hg histedit 'max(desc(D))' --commands - 2>&1 << EOF | fixbundle
  > base cd1f16922537 B
  > drop 3849e69e0651 D
  > pick 5d4ea538b61e X
  > EOF
  $ tglogp
  @  e077fa5e4ecb draft 'X'
  │
  o  cd1f16922537 draft 'B'
  │
  │ o  23a00112b28c draft 'H'
  │ │
  │ │ o  319f51d6224e draft 'G'
  │ ╭─┤
  │ o │  971baba67099 draft 'F'
  ├─╯ │
  │   o  0e89a44ca1b2 draft 'E'
  ├───╯
  o  7b3f3d5e5faf draft 'A'
  

base on a previously picked changeset
  $ echo i > i
  $ hg add i
  $ hg commit -m "I"
  $ echo j > j
  $ hg add j
  $ hg commit -m "J"
  $ tglogp
  @  5aeb8c4a279f draft 'J'
  │
  o  e396a69a02fe draft 'I'
  │
  o  e077fa5e4ecb draft 'X'
  │
  o  cd1f16922537 draft 'B'
  │
  │ o  23a00112b28c draft 'H'
  │ │
  │ │ o  319f51d6224e draft 'G'
  │ ╭─┤
  │ o │  971baba67099 draft 'F'
  ├─╯ │
  │   o  0e89a44ca1b2 draft 'E'
  ├───╯
  o  7b3f3d5e5faf draft 'A'
  
  $ hg histedit 'max(desc(B))' --commands - 2>&1 << EOF | fixbundle
  > pick cd1f16922537 B
  > pick e077fa5e4ecb X
  > base cd1f16922537 B
  > pick 267942e061c5 J
  > base cd1f16922537 B
  > pick fcf8c295f0a2 I
  > EOF
  hg: parse error: base "cd1f16922537" changeset was an edited list candidate
  (base must only use unlisted changesets)

  $ tglogp
  @  5aeb8c4a279f draft 'J'
  │
  o  e396a69a02fe draft 'I'
  │
  o  e077fa5e4ecb draft 'X'
  │
  o  cd1f16922537 draft 'B'
  │
  │ o  23a00112b28c draft 'H'
  │ │
  │ │ o  319f51d6224e draft 'G'
  │ ╭─┤
  │ o │  971baba67099 draft 'F'
  ├─╯ │
  │   o  0e89a44ca1b2 draft 'E'
  ├───╯
  o  7b3f3d5e5faf draft 'A'
  
