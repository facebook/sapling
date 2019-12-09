#chg-compatible

  $ setconfig extensions.treemanifest=!
# Create server
  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > extension=$TESTDIR/flagprocessorext.py
  > EOF
  $ cd ../

# Clone server and enable extensions
  $ hg clone -q server client
  $ cd client
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > extension=$TESTDIR/flagprocessorext.py
  > EOF

# Commit file that will trigger the noop extension
  $ echo '[NOOP]' > noop
  $ hg commit -Aqm "noop"

# Commit file that will trigger the base64 extension
  $ echo '[BASE64]' > base64
  $ hg commit -Aqm 'base64'

# Commit file that will trigger the gzip extension
  $ echo '[GZIP]' > gzip
  $ hg commit -Aqm 'gzip'

# Commit file that will trigger noop and base64
  $ echo '[NOOP][BASE64]' > noop-base64
  $ hg commit -Aqm 'noop+base64'

# Commit file that will trigger noop and gzip
  $ echo '[NOOP][GZIP]' > noop-gzip
  $ hg commit -Aqm 'noop+gzip'

# Commit file that will trigger base64 and gzip
  $ echo '[BASE64][GZIP]' > base64-gzip
  $ hg commit -Aqm 'base64+gzip'

# Commit file that will trigger base64, gzip and noop
  $ echo '[BASE64][GZIP][NOOP]' > base64-gzip-noop
  $ hg commit -Aqm 'base64+gzip+noop'

# TEST: ensure the revision data is consistent
  $ hg cat noop
  [NOOP]
  $ hg debugdata noop 0
  [NOOP]

  $ hg cat -r . base64
  [BASE64]
  $ hg debugdata base64 0
  W0JBU0U2NF0K (no-eol)

  $ hg cat -r . gzip
  [GZIP]
  $ hg debugdata gzip 0
  x\x9c\x8bv\x8f\xf2\x0c\x88\xe5\x02\x00\x08\xc8\x01\xfd (no-eol) (esc)

  $ hg cat -r . noop-base64
  [NOOP][BASE64]
  $ hg debugdata noop-base64 0
  W05PT1BdW0JBU0U2NF0K (no-eol)

  $ hg cat -r . noop-gzip
  [NOOP][GZIP]
  $ hg debugdata noop-gzip 0
  x\x9c\x8b\xf6\xf3\xf7\x0f\x88\x8dv\x8f\xf2\x0c\x88\xe5\x02\x00\x1dH\x03\xf1 (no-eol) (esc)

  $ hg cat -r . base64-gzip
  [BASE64][GZIP]
  $ hg debugdata base64-gzip 0
  eJyLdnIMdjUziY12j/IMiOUCACLBBDo= (no-eol)

  $ hg cat -r . base64-gzip-noop
  [BASE64][GZIP][NOOP]
  $ hg debugdata base64-gzip-noop 0
  eJyLdnIMdjUziY12j/IMiI328/cPiOUCAESjBi4= (no-eol)

# Push to the server
  $ hg push
  pushing to $TESTTMP/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 7 changesets with 7 changes to 7 files

# Initialize new client (not cloning) and setup extension
  $ cd ..
  $ hg init client2
  $ cd client2
  $ cat >> .hg/hgrc << EOF
  > [paths]
  > default = $TESTTMP/server
  > [extensions]
  > extension=$TESTDIR/flagprocessorext.py
  > EOF

# Pull from server and update to latest revision
  $ hg pull default
  pulling from $TESTTMP/server
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 7 changesets with 7 changes to 7 files
  new changesets 07b1b9442c5b:6e48f4215d24
  $ hg update
  7 files updated, 0 files merged, 0 files removed, 0 files unresolved

# TEST: ensure the revision data is consistent
  $ hg cat noop
  [NOOP]
  $ hg debugdata noop 0
  [NOOP]

  $ hg cat -r . base64
  [BASE64]
  $ hg debugdata base64 0
  W0JBU0U2NF0K (no-eol)

  $ hg cat -r . gzip
  [GZIP]
  $ hg debugdata gzip 0
  x\x9c\x8bv\x8f\xf2\x0c\x88\xe5\x02\x00\x08\xc8\x01\xfd (no-eol) (esc)

  $ hg cat -r . noop-base64
  [NOOP][BASE64]
  $ hg debugdata noop-base64 0
  W05PT1BdW0JBU0U2NF0K (no-eol)

  $ hg cat -r . noop-gzip
  [NOOP][GZIP]
  $ hg debugdata noop-gzip 0
  x\x9c\x8b\xf6\xf3\xf7\x0f\x88\x8dv\x8f\xf2\x0c\x88\xe5\x02\x00\x1dH\x03\xf1 (no-eol) (esc)

  $ hg cat -r . base64-gzip
  [BASE64][GZIP]
  $ hg debugdata base64-gzip 0
  eJyLdnIMdjUziY12j/IMiOUCACLBBDo= (no-eol)

  $ hg cat -r . base64-gzip-noop
  [BASE64][GZIP][NOOP]
  $ hg debugdata base64-gzip-noop 0
  eJyLdnIMdjUziY12j/IMiI328/cPiOUCAESjBi4= (no-eol)

# TEST: ensure a missing processor is handled
  $ echo '[FAIL][BASE64][GZIP][NOOP]' > fail-base64-gzip-noop
  $ hg commit -Aqm 'fail+base64+gzip+noop'
  abort: missing processor for flag '0x1'!
  [255]
  $ rm fail-base64-gzip-noop

# TEST: ensure we cannot register several flag processors on the same flag
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > extension=$TESTDIR/flagprocessorext.py
  > duplicate=$TESTDIR/flagprocessorext.py
  > EOF
  $ hg debugrebuilddirstate 2>&1 | grep 'multiple processors'
  Abort: cannot register multiple processors on flag '0x8'.
  *** failed to set up extension duplicate: cannot register multiple processors on flag '0x8'.
  $ hg st 2>&1 | egrep 'cannot register multiple processors|flagprocessorext'
    File "*/tests/flagprocessorext.py", line *, in extsetup (glob)
  Abort: cannot register multiple processors on flag '0x8'.
  *** failed to set up extension duplicate: cannot register multiple processors on flag '0x8'.
    File "*/tests/flagprocessorext.py", line *, in b64decode (glob)

  $ cd ..

# TEST: bundle repo
  $ hg init bundletest
  $ cd bundletest

  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > flagprocessor=$TESTDIR/flagprocessorext.py
  > EOF

  $ for i in 0 single two three 4; do
  >   echo '[BASE64]a-bit-longer-'$i > base64
  >   hg commit -m base64-$i -A base64
  > done

  $ hg update 2 -q
  $ echo '[BASE64]a-bit-longer-branching' > base64
  $ hg commit -q -m branching

  $ hg bundle --base 1 bundle.hg
  4 changesets found
  $ hg debugstrip -r 2 --no-backup --force -q
  $ hg -R bundle.hg log --stat -T '{rev} {desc}\n' base64
  5 branching
   base64 |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
  
  4 base64-4
   base64 |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
  
  3 base64-three
   base64 |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
  
  2 base64-two
   base64 |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
  
  1 base64-single
   base64 |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
  
  0 base64-0
   base64 |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  

  $ hg bundle -R bundle.hg --base 1 bundle-again.hg -q
  $ hg -R bundle-again.hg log --stat -T '{rev} {desc}\n' base64
  5 branching
   base64 |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
  
  4 base64-4
   base64 |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
  
  3 base64-three
   base64 |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
  
  2 base64-two
   base64 |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
  
  1 base64-single
   base64 |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
  
  0 base64-0
   base64 |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ rm bundle.hg bundle-again.hg

# TEST: hg status

  $ hg status
  $ hg diff
