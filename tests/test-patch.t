  $ cat > patchtool.py <<EOF
  > import sys
  > print 'Using custom patch'
  > if '--binary' in sys.argv:
  >     print '--binary found !'
  > EOF

  $ echo "[ui]" >> $HGRCPATH
  $ echo "patch=python ../patchtool.py" >> $HGRCPATH

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg commit -Ama -d '1 0'
  adding a
  $ echo b >> a
  $ hg commit -Amb -d '2 0'
  $ cd ..

This test check that:
 - custom patch commands with arguments actually works
 - patch code does not try to add weird arguments like
 --binary when custom patch commands are used. For instance
 --binary is added by default under win32.

check custom patch options are honored

  $ hg --cwd a export -o ../a.diff tip
  $ hg clone -r 0 a b
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg --cwd b import -v ../a.diff
  applying ../a.diff
  Using custom patch

