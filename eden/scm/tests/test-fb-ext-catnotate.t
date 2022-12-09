#chg-compatible
#debugruntest-compatible

Test catnotate

  $ enable catnotate
  $ hg init repo1
  $ cd repo1

  $ cat > a <<EOF
  > Hell
  > on
  > world
  > EOF

  $ cat > b <<EOF
  > Hello
  > world
  > EOF
  $ printf "\0\n" >> b

  $ hg add a b
  $ hg commit -m "Hello :)"
  $ hg catnotate a b
  a:1: Hell
  a:2: on
  a:3: world
  a:4: 
  b: binary file

  $ hg catnotate -a a b
  a:1: Hell
  a:2: on
  a:3: world
  a:4: 
  b:1: Hello
  b:2: world
  b:3: \x00 (esc)
  b:4: 

  $ hg goto 'desc(Hello)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg catnotate --rev . a
  a:1: Hell
  a:2: on
  a:3: world
  a:4: 
