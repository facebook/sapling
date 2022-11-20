#debugruntest-compatible

  $ enable schemes remotenames
  $ configure modern

  $ cat >> "$HGRCPATH" << EOF
  > [schemes]
  > foo = eager://$TESTTMP/
  > [remotenames]
  > selectivepulldefault = master, stable
  > EOF

  $ newrepo
  $ echo 'A..C' | drawdag
  $ hg path -a default "foo://server1"
  $ hg push -q --to master --create -r $C
  $ hg push -q --to stable --create -r $B

  $ hg bookmarks --remote
     remote/master                    26805aba1e600a82e93661149f2313866a221a7b
     remote/stable                    112478962961147124edd43549aedd1a335e44bf
