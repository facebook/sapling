test sparse

  $ hg init myrepo
  $ cd myrepo
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > sparse=$TESTDIR/../hgext3rd/sparse.py
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

Import a rules file against a 'blank' sparse profile

  $ cat > $HGTMP/rules_to_import <<EOF
  > [include]
  > *.py
  > EOF
  $ hg sparse --import-rules $HGTMP/rules_to_import
  $ ls
  data.py

  $ hg sparse --reset
  $ rm .hg/sparse

  $ cat > $HGTMP/rules_to_import <<EOF
  > %include base.sparse
  > [include]
  > *.py
  > EOF
  $ hg sparse --import-rules $HGTMP/rules_to_import
  $ ls
  base.sparse
  data.py
  webpage.sparse

  $ hg sparse --reset
  $ rm .hg/sparse

Start against an existing profile; rules *already active* should be ignored

  $ hg sparse --enable-profile webpage.sparse
  $ hg sparse --include *.py
  $ cat > $HGTMP/rules_to_import <<EOF
  > %include base.sparse
  > [include]
  > *.html
  > *.txt
  > [exclude]
  > *.py
  > EOF
  $ hg sparse --import-rules $HGTMP/rules_to_import
  $ ls
  base.sparse
  index.html
  readme.txt
  webpage.sparse
  $ cat .hg/sparse
  %include webpage.sparse
  [include]
  *.py
  *.txt
  [exclude]
  *.py

If importing results in no new rules being added, no refresh should take place!

  $ cat > $HGTMP/trap_sparse_refresh.py <<EOF
  > from mercurial import error, extensions
  > def extsetup(ui):
  >     def abort_refresh(ui, *args):
  >         raise error.Abort('sparse._refresh called!')
  >     def sparseloaded(loaded):
  >         if not loaded:
  >             return
  >         sparse = extensions.find('sparse')
  >         sparse._refresh = abort_refresh
  >     extensions.afterloaded('sparse', sparseloaded)
  > EOF
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > trap_sparse_refresh=$HGTMP/trap_sparse_refresh.py
  > EOF
  $ cat > $HGTMP/rules_to_import <<EOF
  > [include]
  > *.py
  > EOF
  $ hg sparse --import-rules $HGTMP/rules_to_import
