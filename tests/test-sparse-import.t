test sparse

  $ enable sparse purge rebase
  $ hg init myrepo
  $ cd myrepo

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

  $ cat > $TESTTMP/rules_to_import <<EOF
  > [include]
  > *.py
  > EOF
  $ hg sparse --import-rules $TESTTMP/rules_to_import
  $ ls
  data.py

  $ hg sparse reset
  $ rm .hg/sparse

  $ cat > $TESTTMP/rules_to_import <<EOF
  > %include base.sparse
  > [include]
  > *.py
  > EOF
  $ hg sparse --import-rules $TESTTMP/rules_to_import
  $ ls
  base.sparse
  data.py
  webpage.sparse

  $ hg sparse reset
  $ rm .hg/sparse

Start against an existing profile; rules *already active* should be ignored

  $ hg sparse --enable-profile webpage.sparse
  $ hg sparse --include *.py
  $ cat > $TESTTMP/rules_to_import <<EOF
  > %include base.sparse
  > [include]
  > *.html
  > *.txt
  > [exclude]
  > *.py
  > EOF
  $ hg sparse --import-rules $TESTTMP/rules_to_import
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

  $ hg sparse reset
  $ rm .hg/sparse

Same tests, with -Tjson enabled to output summaries

  $ cat > $TESTTMP/rules_to_import <<EOF
  > [include]
  > *.py
  > EOF
  $ hg sparse --import-rules $TESTTMP/rules_to_import -Tjson
  [
   {
    "exclude_rules_added": 0,
    "files_added": 0,
    "files_conflicting": 0,
    "files_dropped": 4,
    "include_rules_added": 1,
    "profiles_added": 0
   }
  ]

  $ hg sparse reset
  $ rm .hg/sparse

  $ cat > $TESTTMP/rules_to_import <<EOF
  > %include base.sparse
  > [include]
  > *.py
  > EOF
  $ hg sparse --import-rules $TESTTMP/rules_to_import -Tjson
  [
   {
    "exclude_rules_added": 0,
    "files_added": 0,
    "files_conflicting": 0,
    "files_dropped": 2,
    "include_rules_added": 1,
    "profiles_added": 1
   }
  ]

  $ hg sparse reset
  $ rm .hg/sparse

  $ hg sparse --enable-profile webpage.sparse
  $ hg sparse --include *.py
  $ cat > $TESTTMP/rules_to_import <<EOF
  > %include base.sparse
  > [include]
  > *.html
  > *.txt
  > [exclude]
  > *.py
  > EOF
  $ hg sparse --import-rules $TESTTMP/rules_to_import -Tjson
  [
   {
    "exclude_rules_added": 1,
    "files_added": 1,
    "files_conflicting": 0,
    "files_dropped": 1,
    "include_rules_added": 1,
    "profiles_added": 0
   }
  ]

If importing results in no new rules being added, no refresh should take place!

  $ cat > $TESTTMP/trap_sparse_refresh.py <<EOF
  > from edenscm.mercurial import error, extensions
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
  > trap_sparse_refresh=$TESTTMP/trap_sparse_refresh.py
  > EOF
  $ cat > $TESTTMP/rules_to_import <<EOF
  > [include]
  > *.py
  > EOF
  $ hg sparse --import-rules $TESTTMP/rules_to_import

If an exception is raised during refresh, restore the existing rules again.

  $ cat > $TESTTMP/rules_to_import <<EOF
  > [exclude]
  > *.html
  > EOF
  $ hg sparse --import-rules $TESTTMP/rules_to_import
  abort: sparse._refresh called!
  [255]
  $ cat .hg/sparse
  %include webpage.sparse
  [include]
  *.py
  *.txt
  [exclude]
  *.py
