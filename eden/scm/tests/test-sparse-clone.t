#chg-compatible

  $ disable treemanifest
test sparse

  $ configure dummyssh
  $ setconfig ui.username="nobody <no.reply@fb.com>"
  $ enable sparse purge rebase

  $ hg init myrepo
  $ cd myrepo
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
  $ cd ..

Verify local clone with a sparse profile works

  $ hg clone --enable-profile webpage.sparse myrepo clone1
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd clone1
  $ ls
  index.html
  $ cd ..

Verify local clone with include works

  $ hg clone --include *.sparse myrepo clone2
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd clone2
  $ ls
  backend.sparse
  webpage.sparse
  $ cd ..

Verify local clone with exclude works

  $ hg clone --exclude data.py myrepo clone3
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd clone3
  $ ls
  backend.sparse
  index.html
  readme.txt
  webpage.sparse
  $ cd ..

Verify sparse clone profile over ssh works

  $ hg clone -q --enable-profile webpage.sparse ssh://user@dummy/myrepo clone4
  $ cd clone4
  $ ls
  index.html
  $ cd ..

Verify sparse clone with a non-existing sparse profile warns

  $ hg clone --enable-profile nonexisting.sparse myrepo clone5
  updating to branch default
  the profile 'nonexisting.sparse' does not exist in the current commit, it will only take effect when you check out a commit containing a profile with that name
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd clone5
  $ ls
  backend.sparse
  data.py
  index.html
  readme.txt
  webpage.sparse
  $ cd ..
