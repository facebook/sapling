#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ newrepo
  $ setconfig 'ui.gitignore=1' 'ui.hgignore=0'

  $ cat > .gitignore << 'EOF'
  > *.tmp
  > build/
  > EOF

  $ mkdir build exp
  $ cat > build/.gitignore << 'EOF'
  > !*
  > EOF

  $ cat > exp/.gitignore << 'EOF'
  > !i.tmp
  > EOF

  $ touch build/libfoo.so t.tmp Makefile exp/x.tmp exp/i.tmp

  $ hg status
  ? .gitignore
  ? Makefile
  ? exp/.gitignore
  ? exp/i.tmp

# Test global ignore files

  $ cat > $TESTTMP/globalignore << 'EOF'
  > *.pyc
  > EOF

  $ touch x.pyc

  $ hg status
  ? .gitignore
  ? Makefile
  ? exp/.gitignore
  ? exp/i.tmp
  ? x.pyc

  $ hg status --config 'ui.ignore.global=$TESTTMP/globalignore'
  ? .gitignore
  ? Makefile
  ? exp/.gitignore
  ? exp/i.tmp

# Test directory patterns only match directories.

  $ cat > .gitignore << 'EOF'
  > *.tmp
  > build*/
  > EOF

  $ mkdir buildstuff

  $ touch buildstuff/output builddocs.txt

  $ hg status
  ? .gitignore
  ? Makefile
  ? builddocs.txt
  ? exp/.gitignore
  ? exp/i.tmp
  ? x.pyc
