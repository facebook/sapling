#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

Prevent adding new files in the root directory accidentally.

  $ testrepohg files 'glob:*'
  .editorconfig
  .flake8
  .gitignore
  .hg-vendored-crates
  .hgsigs
  .jshintrc
  .watchmanconfig
  CONTRIBUTING
  CONTRIBUTORS
  COPYING
  Makefile
  README.rst
  TARGETS
  gen_version.py
  hg
  hgeditor
  hgweb.cgi
  setup.py
  vendorcrates.py
