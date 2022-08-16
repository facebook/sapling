#chg-compatible
#debugruntest-compatible

  $ setconfig workingcopy.ruststatus=False
  $ setconfig format.dirstate=2

------ Test dirstate._dirs refcounting

  $ hg init t
  $ cd t
  $ mkdir -p a/b/c/d
  $ touch a/b/c/d/x
  $ touch a/b/c/d/y
  $ touch a/b/c/d/z
  $ hg ci -Am m
  adding a/b/c/d/x
  adding a/b/c/d/y
  adding a/b/c/d/z
  $ hg mv a z
  moving a/b/c/d/x to z/b/c/d/x
  moving a/b/c/d/y to z/b/c/d/y
  moving a/b/c/d/z to z/b/c/d/z

Test name collisions

  $ rm z/b/c/d/x
  $ mkdir z/b/c/d/x
  $ touch z/b/c/d/x/y
  $ hg add z/b/c/d/x/y
  abort: file 'z/b/c/d/x' in dirstate clashes with 'z/b/c/d/x/y'
  [255]
  $ rm -rf z/b/c/d
  $ touch z/b/c/d
  $ hg add z/b/c/d
  abort: directory 'z/b/c/d' already in dirstate
  [255]

  $ cd ..

Issue1790: dirstate entry locked into unset if file mtime is set into
the future

Prepare test repo:

  $ hg init u
  $ cd u
  $ echo a > a
  $ hg add
  adding a
  $ hg ci -m1

Test modulo storage/comparison of absurd dates:

#if no-aix
  $ touch -t 195001011200 a
  $ hg st
  $ hg debugstate
  n 644          2 2018-01-19 15:14:08 a
#endif

Verify that exceptions during a dirstate change leave the dirstate
coherent (issue4353)

  $ cat > ../dirstateexception.py <<EOF
  > from __future__ import absolute_import
  > from edenscm.mercurial import (
  >   error,
  >   extensions,
  >   merge,
  > )
  > 
  > def wraprecordupdates(orig, repo, actions, branchmerge):
  >     raise error.Abort("simulated error while recording dirstateupdates")
  > 
  > def reposetup(ui, repo):
  >     extensions.wrapfunction(merge, 'recordupdates', wraprecordupdates)
  > EOF

  $ hg rm a
  $ hg commit -m 'rm a'
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "dirstateex=../dirstateexception.py" >> .hg/hgrc
  $ hg up 'desc(1)'
  abort: simulated error while recording dirstateupdates
  [255]
  $ hg log -r . -T '{node}\n'
  dfda8c2e7522c4207035f267703c5f27af5a5bf7
  $ hg status
  ? a
  $ rm .hg/hgrc

Verify that status reports deleted files correctly
  $ hg add a
  $ rm a
  $ hg status
  ! a
  $ hg diff

Dirstate should block addition of paths with relative parent components
  $ hg up -C .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls
  $ hg debugsh -c "repo.dirstate.add('foo/../b')"
  abort: cannot add path with relative parents: foo/../b
  [255]
  $ touch b
  $ mkdir foo
  $ hg add foo/../b
  $ hg commit -m "add b"
  $ hg status --change .
  A b
