test sparse

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$(dirname $TESTDIR)/sparse.py
  > purge=
  > strip=
  > rebase=
  > EOF

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

  $ hg sparse --include '*.sparse'

Verify enabling a single profile works

  $ hg sparse --enable-profile webpage.sparse
  $ ls
  backend.sparse
  index.html
  webpage.sparse

Verify enabling two profiles works

  $ hg sparse --enable-profile backend.sparse
  $ ls
  backend.sparse
  data.py
  index.html
  webpage.sparse

Verify disabling a profile works

  $ hg sparse --disable-profile webpage.sparse
  $ ls
  backend.sparse
  data.py
  webpage.sparse


Verify that a profile is updated across multiple commits

  $ cat > webpage.sparse <<EOF
  > [include]
  > *.html
  > EOF
  $ cat > backend.sparse <<EOF
  > [include]
  > *.py
  > *.txt
  > EOF

  $ hg ci -m 'edit profile'
  $ ls
  backend.sparse
  data.py
  readme.txt
  webpage.sparse

  $ hg up -q 0
  $ ls
  backend.sparse
  data.py
  webpage.sparse

  $ hg up -q 1
  $ ls
  backend.sparse
  data.py
  readme.txt
  webpage.sparse

Introduce a conflicting .hgsparse change

  $ hg up -q 0
  $ cat > backend.sparse <<EOF
  > [include]
  > *.html
  > EOF

  $ hg ci -qAm "edit profile other"
  $ ls
  backend.sparse
  index.html
  webpage.sparse

Verify conflicting merge unions the parent profiles

  $ hg merge 1
  merging backend.sparse
  warning: conflicts during merge.
  merging backend.sparse incomplete! (edit conflicts, then use 'hg resolve --mark')
  2 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ rm *.orig
  $ ls
  backend.sparse
  data.py
  index.html
  readme.txt
  webpage.sparse

Verify resolving the merge removes the temporarily unioned files
(*.py in this case)

  $ cat > backend.sparse <<EOF
  > [include]
  > *.html
  > *.txt
  > EOF

  $ hg resolve -m backend.sparse
  (no more unresolved files)

  $ hg ci -qAm "merge profiles"
  $ ls
  backend.sparse
  index.html
  readme.txt
  webpage.sparse

Verify stripping refreshes dirstate

  $ hg strip -q -r .
  $ ls
  backend.sparse
  index.html
  webpage.sparse

Verify rebase conflicts unions parent profiles too

  $ hg rebase -d 1
  rebasing 2:348a944c437a "edit profile other" (tip)
  merging backend.sparse
  warning: conflicts during merge.
  merging backend.sparse incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ rm *.orig
  $ ls
  backend.sparse
  data.py
  index.html
  readme.txt
  webpage.sparse

Verify resolving conflict removes the temporary union too

  $ cat > backend.sparse <<EOF
  > [include]
  > *.html
  > *.txt
  > EOF

  $ hg resolve -m backend.sparse
  (no more unresolved files)

  $ hg rebase -q --continue
  $ ls
  backend.sparse
  index.html
  readme.txt
  webpage.sparse
