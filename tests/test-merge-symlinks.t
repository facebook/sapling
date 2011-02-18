
  $ cat > echo.py <<EOF
  > #!/usr/bin/env python
  > import os, sys
  > try:
  >     import msvcrt
  >     msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
  >     msvcrt.setmode(sys.stderr.fileno(), os.O_BINARY)
  > except ImportError:
  >     pass
  > 
  > for k in ('HG_FILE', 'HG_MY_ISLINK', 'HG_OTHER_ISLINK', 'HG_BASE_ISLINK'):
  >     print k, os.environ[k]
  > EOF

Create 2 heads containing the same file, once as
a file, once as a link. Bundle was generated with:

# hg init t
# cd t
# echo a > a
# hg ci -qAm t0 -d '0 0'
# echo l > l
# hg ci -qAm t1 -d '1 0'
# hg up -C 0
# ln -s a l
# hg ci -qAm t2 -d '2 0'
# echo l2 > l2
# hg ci -qAm t3 -d '3 0'

  $ hg init t
  $ cd t
  $ hg -q pull "$TESTDIR/test-merge-symlinks.hg"
  $ hg up -C 3
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Merge them and display *_ISLINK vars
merge heads

  $ hg merge --tool="python ../echo.py"
  merging l
  HG_FILE l
  HG_MY_ISLINK 1
  HG_OTHER_ISLINK 0
  HG_BASE_ISLINK 0
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Test working directory symlink bit calculation wrt copies,
especially on non-supporting systems.
merge working directory

  $ hg up -C 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg copy l l2
  $ HGMERGE="python ../echo.py" hg up 3
  merging l2
  HG_FILE l2
  HG_MY_ISLINK 1
  HG_OTHER_ISLINK 0
  HG_BASE_ISLINK 0
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
