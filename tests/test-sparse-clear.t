test sparse

  $ hg init myrepo
  $ cd myrepo
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > sparse=
  > purge=
  > strip=
  > rebase=
  > EOF

  $ echo a > index.html
  $ echo x > data.py
  $ echo z > readme.txt
  $ cat > base.sparse <<EOF
  > [include]
  > *.sparse
  > EOF
  $ hg ci -Aqm 'initial'
  $ cat > webpage.sparse <<EOF
  > %include base.sparse
  > [include]
  > *.html
  > EOF
  $ hg ci -Aqm 'initial'

Clear rules when there are includes

  $ hg debugsparse --include *.py
  $ ls
  data.py
  $ hg debugsparse --clear-rules
  $ ls
  base.sparse
  data.py
  index.html
  readme.txt
  webpage.sparse

Clear rules when there are excludes

  $ hg debugsparse --exclude *.sparse
  $ ls
  data.py
  index.html
  readme.txt
  $ hg debugsparse --clear-rules
  $ ls
  base.sparse
  data.py
  index.html
  readme.txt
  webpage.sparse

Clearing rules should not alter profiles

  $ hg debugsparse --enable-profile webpage.sparse
  $ ls
  base.sparse
  index.html
  webpage.sparse
  $ hg debugsparse --include *.py
  $ ls
  base.sparse
  data.py
  index.html
  webpage.sparse
  $ hg debugsparse --clear-rules
  $ ls
  base.sparse
  index.html
  webpage.sparse
