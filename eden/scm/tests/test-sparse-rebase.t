#chg-compatible

  $ hg init repo
  $ cd repo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > rebase=
  > EOF

  $ hg debugdrawdag <<'EOS'
  >   D
  >   |
  > B C
  > |/
  > A
  > EOS

  $ hg sparse --exclude A B C D E
  $ hg update A -q
  $ printf D > D
  $ echo 2 > E
  $ hg rebase -s C -d B
  rebasing dc0947a82db8 "C" (C)
  temporarily included 1 file(s) in the sparse checkout for merging
  cleaned up 1 temporarily added file(s) from the sparse checkout
  rebasing e7b3f00ed42e "D" (D tip)
  temporarily included 1 file(s) in the sparse checkout for merging
  cleaned up 1 temporarily added file(s) from the sparse checkout
