#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Tests for the automv extension; detect moved files at commit time.

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > automv=
  > rebase=
  > EOF

# Setup repo

  $ hg init repo
  $ cd repo

# Test automv command for commit

  $ printf 'foo\nbar\nbaz\n' > a.txt
  $ hg add a.txt
  $ hg commit -m 'init repo with a'
  $ hg bookmark -i rev0

# mv/rm/add

  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit -m msg
  detected move of 1 files
  $ hg status --change . -C
  A b.txt
    a.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

# mv/rm/add/modif

  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ printf '\n' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit -m msg
  detected move of 1 files
  $ hg status --change . -C
  A b.txt
    a.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

# mv/rm/add/modif

  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ printf '\nfoo\n' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit -m msg
  $ hg status --change . -C
  A b.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

# mv/rm/add/modif/changethreshold

  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ printf '\nfoo\n' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --config 'automv.similarity=60' -m msg
  detected move of 1 files
  $ hg status --change . -C
  A b.txt
    a.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

# mv

  $ mv a.txt b.txt
  $ hg status -C
  ! a.txt
  ? b.txt
  $ hg commit -m msg
  nothing changed (1 missing files, see 'hg status')
  [1]
  $ hg status -C
  ! a.txt
  ? b.txt
  $ hg revert -aqC
  $ rm b.txt

# mv/rm/add/notincommitfiles

  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ echo bar > c.txt
  $ hg add c.txt
  $ hg status -C
  A b.txt
  A c.txt
  R a.txt
  $ hg commit c.txt -m msg
  $ hg status --change . -C
  A c.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg rm a.txt
  $ echo bar > c.txt
  $ hg add c.txt
  $ hg commit -m msg
  detected move of 1 files
  $ hg status --change . -C
  A b.txt
    a.txt
  A c.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

# mv/rm/add/--no-automv

  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --no-automv -m msg
  $ hg status --change . -C
  A b.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

# Test automv command for commit --amend
# mv/rm/add

  $ echo c > c.txt
  $ hg add c.txt
  $ hg commit -m 'revision to amend to'
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --amend -m amended
  detected move of 1 files
  $ hg status --change . -C
  A b.txt
    a.txt
  A c.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

# mv/rm/add/modif

  $ echo c > c.txt
  $ hg add c.txt
  $ hg commit -m 'revision to amend to'
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ printf '\n' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --amend -m amended
  detected move of 1 files
  $ hg status --change . -C
  A b.txt
    a.txt
  A c.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

# mv/rm/add/modif

  $ echo c > c.txt
  $ hg add c.txt
  $ hg commit -m 'revision to amend to'
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ printf '\nfoo\n' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --amend -m amended
  $ hg status --change . -C
  A b.txt
  A c.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

# mv/rm/add/modif/changethreshold

  $ echo c > c.txt
  $ hg add c.txt
  $ hg commit -m 'revision to amend to'
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ printf '\nfoo\n' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --amend --config 'automv.similarity=60' -m amended
  detected move of 1 files
  $ hg status --change . -C
  A b.txt
    a.txt
  A c.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

# mv

  $ echo c > c.txt
  $ hg add c.txt
  $ hg commit -m 'revision to amend to'
  $ mv a.txt b.txt
  $ hg status -C
  ! a.txt
  ? b.txt
  $ hg commit --amend -m amended
  $ hg status -C
  ! a.txt
  ? b.txt
  $ hg up -Cr 'min(rev0)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

# mv/rm/add/notincommitfiles

  $ echo c > c.txt
  $ hg add c.txt
  $ hg commit -m 'revision to amend to'
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ echo bar > d.txt
  $ hg add d.txt
  $ hg status -C
  A b.txt
  A d.txt
  R a.txt
  $ hg commit --amend -m amended d.txt
  $ hg status --change . -C
  A c.txt
  A d.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --amend -m amended
  detected move of 1 files
  $ hg status --change . -C
  A b.txt
    a.txt
  A c.txt
  A d.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 3 files removed, 0 files unresolved

# mv/rm/add/--no-automv

  $ echo c > c.txt
  $ hg add c.txt
  $ hg commit -m 'revision to amend to'
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --amend -m amended --no-automv
  $ hg status --change . -C
  A b.txt
  A c.txt
  R a.txt
  $ hg up -r 'min(rev0)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

# mv/rm/commit/add/amend

  $ echo c > c.txt
  $ hg add c.txt
  $ hg commit -m 'revision to amend to'
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg status -C
  R a.txt
  ? b.txt
  $ hg commit -m 'removed a'
  $ hg add b.txt
  $ hg commit --amend -m amended
  $ hg status --change . -C
  A b.txt
  R a.txt

# error conditions

  $ cat >> $HGRCPATH << 'EOF'
  > [automv]
  > similarity=110
  > EOF
  $ hg commit -m 'revision to amend to'
  abort: automv.similarity must be between 0 and 100
  [255]

# max-files setting

  $ newrepo
  $ touch A
  $ hg ci -m A -A A --config automv.similarity=1
  $ mv A B
  $ hg addremove A B
  $ hg ci -m mv --config automv.max-files=0 --config automv.similarity=1
  $ hg status --change . -C
  A B
  R A
