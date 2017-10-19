  $ cat >> $HGRCPATH <<EOF
  > [experimental]
  > bundle-phases=yes
  > [extensions]
  > strip=
  > drawdag=$TESTDIR/drawdag.py
  > EOF

Set up repo with linear history
  $ hg init linear
  $ cd linear
  $ hg debugdrawdag <<'EOF'
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOF
  $ hg phase --public A
  $ hg phase --force --secret D
  $ hg log -G -T '{desc} {phase}\n'
  o  E secret
  |
  o  D secret
  |
  o  C draft
  |
  o  B draft
  |
  o  A public
  
Phases are restored when unbundling
  $ hg bundle --base B -r E bundle
  3 changesets found
  $ hg debugbundle bundle
  Stream params: {Compression: BZ}
  changegroup -- {nbchanges: 3, targetphase: 2, version: 02}
      26805aba1e600a82e93661149f2313866a221a7b
      f585351a92f85104bff7c284233c338b10eb1df7
      9bc730a19041f9ec7cb33c626e811aa233efb18c
  phase-heads -- {}
      26805aba1e600a82e93661149f2313866a221a7b draft
  $ hg strip --no-backup C
  $ hg unbundle -q bundle
  $ rm bundle
  $ hg log -G -T '{desc} {phase}\n'
  o  E secret
  |
  o  D secret
  |
  o  C draft
  |
  o  B draft
  |
  o  A public
  
Root revision's phase is preserved
  $ hg bundle -a bundle
  5 changesets found
  $ hg strip --no-backup A
  $ hg unbundle -q bundle
  $ rm bundle
  $ hg log -G -T '{desc} {phase}\n'
  o  E secret
  |
  o  D secret
  |
  o  C draft
  |
  o  B draft
  |
  o  A public
  
Completely public history can be restored
  $ hg phase --public E
  $ hg bundle -a bundle
  5 changesets found
  $ hg strip --no-backup A
  $ hg unbundle -q bundle
  $ rm bundle
  $ hg log -G -T '{desc} {phase}\n'
  o  E public
  |
  o  D public
  |
  o  C public
  |
  o  B public
  |
  o  A public
  
Direct transition from public to secret can be restored
  $ hg phase --secret --force D
  $ hg bundle -a bundle
  5 changesets found
  $ hg strip --no-backup A
  $ hg unbundle -q bundle
  $ rm bundle
  $ hg log -G -T '{desc} {phase}\n'
  o  E secret
  |
  o  D secret
  |
  o  C public
  |
  o  B public
  |
  o  A public
  
Revisions within bundle preserve their phase even if parent changes its phase
  $ hg phase --draft --force B
  $ hg bundle --base B -r E bundle
  3 changesets found
  $ hg strip --no-backup C
  $ hg phase --public B
  $ hg unbundle -q bundle
  $ rm bundle
  $ hg log -G -T '{desc} {phase}\n'
  o  E secret
  |
  o  D secret
  |
  o  C draft
  |
  o  B public
  |
  o  A public
  
Phase of ancestors of stripped node get advanced to accommodate child
  $ hg bundle --base B -r E bundle
  3 changesets found
  $ hg strip --no-backup C
  $ hg phase --force --secret B
  $ hg unbundle -q bundle
  $ rm bundle
  $ hg log -G -T '{desc} {phase}\n'
  o  E secret
  |
  o  D secret
  |
  o  C draft
  |
  o  B draft
  |
  o  A public
  
Unbundling advances phases of changesets even if they were already in the repo.
To test that, create a bundle of everything in draft phase and then unbundle
to see that secret becomes draft, but public remains public.
  $ hg phase --draft --force A
  $ hg phase --draft E
  $ hg bundle -a bundle
  5 changesets found
  $ hg phase --public A
  $ hg phase --secret --force E
  $ hg unbundle -q bundle
  $ rm bundle
  $ hg log -G -T '{desc} {phase}\n'
  o  E draft
  |
  o  D draft
  |
  o  C draft
  |
  o  B draft
  |
  o  A public
  
Unbundling change in the middle of a stack does not affect later changes
  $ hg strip --no-backup E
  $ hg phase --secret --force D
  $ hg log -G -T '{desc} {phase}\n'
  o  D secret
  |
  o  C draft
  |
  o  B draft
  |
  o  A public
  
  $ hg bundle --base A -r B bundle
  1 changesets found
  $ hg unbundle -q bundle
  $ rm bundle
  $ hg log -G -T '{desc} {phase}\n'
  o  D secret
  |
  o  C draft
  |
  o  B draft
  |
  o  A public
  

  $ cd ..

Set up repo with non-linear history
  $ hg init non-linear
  $ cd non-linear
  $ hg debugdrawdag <<'EOF'
  > D E
  > |\|
  > B C
  > |/
  > A
  > EOF
  $ hg phase --public C
  $ hg phase --force --secret B
  $ hg log -G -T '{node|short} {desc} {phase}\n'
  o  03ca77807e91 E draft
  |
  | o  4e4f9194f9f1 D secret
  |/|
  o |  dc0947a82db8 C public
  | |
  | o  112478962961 B secret
  |/
  o  426bada5c675 A public
  

Restore bundle of entire repo
  $ hg bundle -a bundle
  5 changesets found
  $ hg debugbundle bundle
  Stream params: {Compression: BZ}
  changegroup -- {nbchanges: 5, targetphase: 2, version: 02}
      426bada5c67598ca65036d57d9e4b64b0c1ce7a0
      112478962961147124edd43549aedd1a335e44bf
      dc0947a82db884575bb76ea10ac97b08536bfa03
      4e4f9194f9f181c57f62e823e8bdfa46ab9e4ff4
      03ca77807e919db8807c3749086dc36fb478cac0
  phase-heads -- {}
      dc0947a82db884575bb76ea10ac97b08536bfa03 public
      03ca77807e919db8807c3749086dc36fb478cac0 draft
  $ hg strip --no-backup A
  $ hg unbundle -q bundle
  $ rm bundle
  $ hg log -G -T '{node|short} {desc} {phase}\n'
  o  03ca77807e91 E draft
  |
  | o  4e4f9194f9f1 D secret
  |/|
  o |  dc0947a82db8 C public
  | |
  | o  112478962961 B secret
  |/
  o  426bada5c675 A public
  

  $ hg bundle --base 'A + C' -r D bundle
  2 changesets found
  $ hg debugbundle bundle
  Stream params: {Compression: BZ}
  changegroup -- {nbchanges: 2, targetphase: 2, version: 02}
      112478962961147124edd43549aedd1a335e44bf
      4e4f9194f9f181c57f62e823e8bdfa46ab9e4ff4
  phase-heads -- {}
  $ rm bundle

  $ hg bundle --base A -r D bundle
  3 changesets found
  $ hg debugbundle bundle
  Stream params: {Compression: BZ}
  changegroup -- {nbchanges: 3, targetphase: 2, version: 02}
      112478962961147124edd43549aedd1a335e44bf
      dc0947a82db884575bb76ea10ac97b08536bfa03
      4e4f9194f9f181c57f62e823e8bdfa46ab9e4ff4
  phase-heads -- {}
      dc0947a82db884575bb76ea10ac97b08536bfa03 public
  $ rm bundle

  $ hg bundle --base 'B + C' -r 'D + E' bundle
  2 changesets found
  $ hg debugbundle bundle
  Stream params: {Compression: BZ}
  changegroup -- {nbchanges: 2, targetphase: 2, version: 02}
      4e4f9194f9f181c57f62e823e8bdfa46ab9e4ff4
      03ca77807e919db8807c3749086dc36fb478cac0
  phase-heads -- {}
      03ca77807e919db8807c3749086dc36fb478cac0 draft
  $ rm bundle
