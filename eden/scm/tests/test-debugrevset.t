#chg-compatible

  $ configure modern

Setup repo:
  $ newrepo
  $ drawdag << 'EOS'
  > G
  > |
  > F
  > |
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg log -T "{node}\n"
  43195508e3bb704c08d24c40375bdd826789dd72
  a194cadd16930608adaa649035ad4c16930cbd0f
  9bc730a19041f9ec7cb33c626e811aa233efb18c
  f585351a92f85104bff7c284233c338b10eb1df7
  26805aba1e600a82e93661149f2313866a221a7b
  112478962961147124edd43549aedd1a335e44bf
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0

Test hash prefix lookup:
  $ hg debugrevset 431
  43195508e3bb704c08d24c40375bdd826789dd72
  $ hg debugrevset 4
  abort: ambiguous identifier for '4': 426bada5c67598ca65036d57d9e4b64b0c1ce7a0, 43195508e3bb704c08d24c40375bdd826789dd72 available
  [255]
  $ hg debugrevset 6
  abort: unknown revision '6'
  [255]
  $ hg debugrevset thisshóuldnótbéfoünd
  abort: unknown revision 'thissh*' (glob)
  [255]

Test bookmark lookup
  $ hg book -r 'desc(C)' mybookmark
  $ hg debugrevset mybookmark
  26805aba1e600a82e93661149f2313866a221a7b

Test resolution priority
  $ hg book -r 'desc(A)' f
  $ hg debugrevset f
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
