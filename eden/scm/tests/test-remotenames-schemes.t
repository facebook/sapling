  $ enable schemes remotenames

  $ newrepo a
  $ drawdag << 'EOS'
  > X
  > |
  > Z
  > EOS
  $ hg bookmark -r $X bookmark-X

  $ newrepo b
  $ drawdag << 'EOS'
  > Y
  > |
  > Z
  > EOS
  $ hg bookmark -r $Y bookmark-Y

  $ newrepo c
  $ cat >> .hg/hgrc << EOF
  > [schemes]
  > dotdot = ../{1}
  > EOF

  $ hg pull -q dotdot://a
  $ hg pull -q dotdot://b

  $ hg bookmark --remote


