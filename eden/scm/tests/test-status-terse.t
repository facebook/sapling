#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ mkdir folder
  $ cd folder
  $ hg init
  $ mkdir x x/l x/m x/n x/l/u x/l/u/a
  $ touch a b x/aa.o x/bb.o
  $ hg status
  ? a
  ? b
  ? x/aa.o
  ? x/bb.o

  $ hg status --terse u
  ? a
  ? b
  ? x/
  $ hg status --terse maudric
  ? a
  ? b
  ? x/
  $ hg status --terse madric
  ? a
  ? b
  ? x/aa.o
  ? x/bb.o
  $ hg status --terse f
  abort: 'f' not recognized
  [255]

# Add a .gitignore so that we can also have ignored files

  $ echo '*\.o' > .gitignore
  $ hg status
  ? .gitignore
  ? a
  ? b
  $ hg status -i
  I x/aa.o
  I x/bb.o

# Tersing ignored files

  $ hg status -t i --ignored
  I x/

# Adding more files

  $ mkdir y
  $ touch x/aa x/bb y/l y/m y/l.o y/m.o
  $ touch x/l/aa x/m/aa x/n/aa x/l/u/bb x/l/u/a/bb

  $ hg status
  ? .gitignore
  ? a
  ? b
  ? x/aa
  ? x/bb
  ? x/l/aa
  ? x/l/u/a/bb
  ? x/l/u/bb
  ? x/m/aa
  ? x/n/aa
  ? y/l
  ? y/m

  $ hg status --terse u
  ? .gitignore
  ? a
  ? b
  ? x/
  ? y/

  $ hg add x/aa x/bb .gitignore
  $ hg status --terse au
  A .gitignore
  A x/aa
  A x/bb
  ? a
  ? b
  ? x/l/
  ? x/m/
  ? x/n/
  ? y/

# Including ignored files

  $ hg status --terse aui
  A .gitignore
  A x/aa
  A x/bb
  ? a
  ? b
  ? x/l/
  ? x/m/
  ? x/n/
  ? y/l
  ? y/m
  $ hg status --terse au -i
  I x/aa.o
  I x/bb.o
  I y/l.o
  I y/m.o

# Committing some of the files

  $ hg commit x/aa x/bb .gitignore -m 'First commit'
  $ hg status
  ? a
  ? b
  ? x/l/aa
  ? x/l/u/a/bb
  ? x/l/u/bb
  ? x/m/aa
  ? x/n/aa
  ? y/l
  ? y/m
  $ hg status --terse mardu
  ? a
  ? b
  ? x/l/
  ? x/m/
  ? x/n/
  ? y/

# Modifying already committed files

  $ echo Hello >> x/aa
  $ echo World >> x/bb
  $ hg status --terse maurdc
  M x/aa
  M x/bb
  ? a
  ? b
  ? x/l/
  ? x/m/
  ? x/n/
  ? y/

# Respecting other flags

  $ hg status --terse marduic --all
  M x/aa
  M x/bb
  ? a
  ? b
  ? x/l/
  ? x/m/
  ? x/n/
  ? y/l
  ? y/m
  I x/aa.o
  I x/bb.o
  I y/l.o
  I y/m.o
  C .gitignore
  $ hg status --terse marduic -a
  $ hg status --terse marduic -c
  C .gitignore
  $ hg status --terse marduic -m
  M x/aa
  M x/bb

# Passing 'i' in terse value will consider the ignored files while tersing

  $ hg status --terse marduic -u
  ? a
  ? b
  ? x/l/
  ? x/m/
  ? x/n/
  ? y/l
  ? y/m

# Omitting 'i' in terse value does not consider ignored files while tersing

  $ hg status --terse marduc -u
  ? a
  ? b
  ? x/l/
  ? x/m/
  ? x/n/
  ? y/

# Trying with --rev

  $ hg status --terse marduic --rev 0 --rev 1
  abort: cannot use --terse with --rev
  [255]
