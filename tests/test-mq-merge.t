# Test issue 529 - mq aborts when merging patch deleting files

  $ checkundo()
  > {
  >     if [ -f .hg/store/undo ]; then
  >         echo ".hg/store/undo still exists"
  >     fi
  > }

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq =" >> $HGRCPATH
  $ echo "[mq]" >> $HGRCPATH
  $ echo "git = keep" >> $HGRCPATH

Commit two dummy files in "init" changeset:

  $ hg init t
  $ cd t
  $ echo a > a
  $ echo b > b
  $ hg ci -Am init
  adding a
  adding b
  $ hg tag -l init

Create a patch removing a:

  $ hg qnew rm_a
  $ hg rm a
  $ hg qrefresh -m "rm a"

Save the patch queue so we can merge it later:

  $ hg qsave -c -e
  copy .*/t/.hg/patches to .*/t/.hg/patches.1
  $ checkundo

Update b and commit in an "update" changeset:

  $ hg up -C init
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b >> b
  $ hg st
  M b
  $ hg ci -m update
  created new head

# Here, qpush used to abort with :
# The system cannot find the file specified => a
  $ hg manifest
  a
  b

  $ hg qpush -a -m
  merging with queue at: .*/t/.hg/patches.1
  applying rm_a
  now at: rm_a

  $ checkundo
  $ hg manifest
  b

Ensure status is correct after merge:

  $ hg qpop -a
  popping rm_a
  popping .hg.patches.merge.marker
  patch queue now empty

  $ cd ..

Classic MQ merge sequence *with an explicit named queue*:

  $ hg init t2
  $ cd t2
  $ echo '[diff]' > .hg/hgrc
  $ echo 'nodates = 1' >> .hg/hgrc
  $ echo a > a
  $ hg ci -Am init
  adding a
  $ echo b > a
  $ hg ci -m changea
  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg cp a aa
  $ echo c >> a
  $ hg qnew --git -f -e patcha
  $ echo d >> a
  $ hg qnew -d '0 0' -f -e patcha2

Create the reference queue:

  $ hg qsave -c -e -n refqueue
  copy .*/t2/.hg/patches to .*/t2/.hg/refqueue
  $ hg up -C 1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

Merge:

  $ HGMERGE=internal:other hg qpush -a -m -n refqueue
  merging with queue at: .*/t2/.hg/refqueue
  applying patcha
  patching file a
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file a.rej
  patch failed, unable to continue (try -v)
  patch failed, rejects left in working dir
  patch didn't work out, merging patcha
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  applying patcha2
  now at: patcha2

Check patcha is still a git patch:

  $ cat .hg/patches/patcha
  # HG changeset patch
  # Parent d3873e73d99ef67873dac33fbcc66268d5d2b6f4
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
  -b
  +a
  +c
  diff --git a/a b/aa
  copy from a
  copy to aa
  --- a/a
  +++ b/aa
  @@ -1,1 +1,1 @@
  -b
  +a

Check patcha2 is still a regular patch:

  $ cat .hg/patches/patcha2
  # HG changeset patch
  # Parent ........................................
  # Date 0 0
  
  diff -r ............ -r ............ a
  --- a/a
  +++ b/a
  @@ -1,2 +1,3 @@
   a
   c
  +d

  $ cd ..

