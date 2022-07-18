#chg-compatible
  $ setconfig experimental.allowfilepeer=True clone.use-rust=1 commands.force-rust=clone

test sparse

  $ configure modernclient
  $ setconfig ui.username="nobody <no.reply@fb.com>"
  $ enable sparse rebase

  $ newremoterepo repo1
  $ setconfig paths.default=test:e1
  $ echo a > index.html
  $ echo x > data.py
  $ echo z > readme.txt
  $ cat > webpage.sparse <<EOF
  > [include]
  > *.html
  > EOF
  $ cat > backend.sparse <<EOF
  > [include]
  > *.py
  > EOF
  $ hg ci -Aqm 'initial'
  $ hg push -r . --to master --create -q

Verify local clone with a sparse profile works

  $ cd $TESTTMP
  $ hg clone --enable-profile webpage.sparse test:e1 clone1
  Cloning * into $TESTTMP/clone1 (glob)
  Checking out 'master'
  1 files updated
  $ cd clone1
  $ ls
  index.html
  $ cd ..

Verify sparse clone with a non-existing sparse profile warns

  $ hg clone --enable-profile nonexisting.sparse test:e1 clone5
  Cloning * into $TESTTMP/clone5 (glob)
  Checking out 'master'
  The profile 'nonexisting.sparse' does not exist. Check out a commit where it exists, or remove it with 'hg sparse disableprofile'.
  5 files updated
  $ cd clone5
  $ ls
  backend.sparse
  data.py
  index.html
  readme.txt
  webpage.sparse
  $ cd ..
