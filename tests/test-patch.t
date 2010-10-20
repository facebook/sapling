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

This test checks that:
 - custom patch commands with arguments actually work
 - patch code does not try to add weird arguments like
 --binary when custom patch commands are used. For instance
 --binary is added by default under win32.

check custom patch options are honored

  $ hg --cwd a export -o ../a.diff tip
  $ hg clone -r 0 a b
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg --cwd b import -v ../a.diff
  applying ../a.diff
  Using custom patch


Issue2417: hg import with # comments in description

Prepare source repo and patch:

  $ rm $HGRCPATH
  $ hg init c
  $ cd c
  $ echo 0 > a
  $ hg ci -A -m 0 a -d '0 0'
  $ echo 1 >> a
  $ cat << eof > log
  > first line which can't start with '# '
  > # second line is a comment but that shouldn't be a problem.
  > A patch marker like this was more problematic even after d7452292f9d3:
  > # HG changeset patch
  > # User lines looks like this - but it _is_ just a comment
  > eof
  $ hg ci -l log -d '0 0'
  $ hg export -o p 1
  $ cd ..

Clone and apply patch:

  $ hg clone -r 0 c d
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd d
  $ hg import ../c/p
  applying ../c/p
  $ hg log -v -r 1
  changeset:   1:e8cc66fbbaa6
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  first line which can't start with '# '
  # second line is a comment but that shouldn't be a problem.
  A patch marker like this was more problematic even after d7452292f9d3:
  # HG changeset patch
  # User lines looks like this - but it _is_ just a comment
  
  
  $ cd ..
