#chg-compatible

  $ enable amend rebase reset
  $ setconfig experimental.evolution=obsolete
  $ setconfig visibility.enabled=true
  $ setconfig mutation.record=true mutation.enabled=true mutation.date="0 0"

  $ newrepo
  $ drawdag << EOS
  > E F
  > |/
  > C D
  > |/
  > B
  > |
  > A
  > EOS
  $ hg up -q $E
  $ hg bookmark test-bookmark
  $ tglogm
  o  5: ee481a2a1e69 'F'
  |
  | @  4: 78d2dca436b2 'E' test-bookmark
  |/
  | o  3: be0ef73c17ad 'D'
  | |
  o |  2: 26805aba1e60 'C'
  |/
  o  1: 112478962961 'B'
  |
  o  0: 426bada5c675 'A'
  
  $ hg reset -C $D
  2 changesets hidden

Note that reset tried to hide 'C', but this was ignored because of 'F'.

  $ tglogm
  o  5: ee481a2a1e69 'F'
  |
  | @  3: be0ef73c17ad 'D' test-bookmark
  | |
  o |  2: 26805aba1e60 'C'
  |/
  o  1: 112478962961 'B'
  |
  o  0: 426bada5c675 'A'
  
