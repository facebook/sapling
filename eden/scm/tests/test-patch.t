#chg-compatible
  $ setconfig format.use-segmented-changelog=true
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig experimental.allowfilepeer=True

  $ cat > patchtool.py <<EOF
  > from __future__ import absolute_import, print_function
  > import sys
  > print('Using custom patch')
  > if '--binary' in sys.argv:
  >     print('--binary found !')
  > EOF

  $ setconfig ui.patch="$PYTHON $TESTTMP/patchtool.py"

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
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg --cwd b import -v ../a.diff
  applying ../a.diff
  Using custom patch
  applied to working directory

Issue2417: hg import with # comments in description

Prepare source repo and patch:

  $ cat >> "$HGRCPATH" << EOF
  > [ui]
  > %unset patch
  > EOF
  $ hg init c
  $ cd c
  $ printf "a\rc" > a
  $ hg ci -A -m 0 a -d '0 0'
  $ printf "a\rb\rc" > a
  $ cat << eof > log
  > first line which can't start with '# '
  > # second line is a comment but that shouldn't be a problem.
  > A patch marker like this was more problematic even after d7452292f9d3:
  > # HG changeset patch
  > # User lines looks like this - but it _is_ just a comment
  > eof
  $ hg ci -l log -d '0 0'
  $ hg export -o p 'desc(first)'
  $ cd ..

Clone and apply patch:

  $ setconfig remotefilelog.cachepath=$TESTTMP/cache
  $ hg clone -r 0 c d --config ui.ignorerevnum=0
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd d
  $ hg import ../c/p
  applying ../c/p
  $ hg log -v -r 'desc(first)'
  commit:      cd0bde79c428
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

Test invalid patch
  $ cd d
  $ hg export -r . -o patch.patch
  $ hg up -q .^
  $ hg debugpython <<EOF
  > s = b''.join(open('patch.patch', 'rb').readlines()[:-2])
  > open('patch.patch', 'wb').write(s)
  > EOF
  $ hg import patch.patch
  applying patch.patch
  abort: hunk processing error - hunk too short
  [255]
