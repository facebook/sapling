  $ hg init
  $ mkdir d1 d1/d11 d2
  $ echo d1/a > d1/a
  $ echo d1/ba > d1/ba
  $ echo d1/a1 > d1/d11/a1
  $ echo d1/b > d1/b
  $ echo d2/b > d2/b
  $ hg add d1/a d1/b d1/ba d1/d11/a1 d2/b
  $ hg commit -m "1"

rename a single file

  $ hg rename d1/d11/a1 d2/c
  $ hg --config ui.portablefilenames=abort rename d1/a d1/con.xml
  abort: filename contains 'con', which is reserved on Windows: 'd1/con.xml'
  [255]
  $ hg sum
  parent: 0:9b4b6e7b2c26 tip
   1
  branch: default
  commit: 1 renamed
  update: (current)
  phases: 1 draft
  $ hg status -C
  A d2/c
    d1/d11/a1
  R d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/c

rename a single file using absolute paths

  $ hg rename `pwd`/d1/d11/a1 `pwd`/d2/c
  $ hg status -C
  A d2/c
    d1/d11/a1
  R d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/c

rename --after a single file

  $ mv d1/d11/a1 d2/c
  $ hg rename --after d1/d11/a1 d2/c
  $ hg status -C
  A d2/c
    d1/d11/a1
  R d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/c

rename --after a single file when src and tgt already tracked

  $ mv d1/d11/a1 d2/c
  $ hg addrem -s 0
  removing d1/d11/a1
  adding d2/c
  $ hg rename --after d1/d11/a1 d2/c
  $ hg status -C
  A d2/c
    d1/d11/a1
  R d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/c

rename --after a single file to a nonexistent target filename

  $ hg rename --after d1/a dummy
  d1/a: not recording move - dummy does not exist (glob)

move a single file to an existing directory

  $ hg rename d1/d11/a1 d2
  $ hg status -C
  A d2/a1
    d1/d11/a1
  R d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/a1

move --after a single file to an existing directory

  $ mv d1/d11/a1 d2
  $ hg rename --after d1/d11/a1 d2
  $ hg status -C
  A d2/a1
    d1/d11/a1
  R d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/a1

rename a file using a relative path

  $ (cd d1/d11; hg rename ../../d2/b e)
  $ hg status -C
  A d1/d11/e
    d2/b
  R d2/b
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/d11/e

rename --after a file using a relative path

  $ (cd d1/d11; mv ../../d2/b e; hg rename --after ../../d2/b e)
  $ hg status -C
  A d1/d11/e
    d2/b
  R d2/b
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/d11/e

rename directory d1 as d3

  $ hg rename d1/ d3
  moving d1/a to d3/a (glob)
  moving d1/b to d3/b (glob)
  moving d1/ba to d3/ba (glob)
  moving d1/d11/a1 to d3/d11/a1 (glob)
  $ hg status -C
  A d3/a
    d1/a
  A d3/b
    d1/b
  A d3/ba
    d1/ba
  A d3/d11/a1
    d1/d11/a1
  R d1/a
  R d1/b
  R d1/ba
  R d1/d11/a1
  $ hg update -C
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

rename --after directory d1 as d3

  $ mv d1 d3
  $ hg rename --after d1 d3
  moving d1/a to d3/a (glob)
  moving d1/b to d3/b (glob)
  moving d1/ba to d3/ba (glob)
  moving d1/d11/a1 to d3/d11/a1 (glob)
  $ hg status -C
  A d3/a
    d1/a
  A d3/b
    d1/b
  A d3/ba
    d1/ba
  A d3/d11/a1
    d1/d11/a1
  R d1/a
  R d1/b
  R d1/ba
  R d1/d11/a1
  $ hg update -C
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move a directory using a relative path

  $ (cd d2; mkdir d3; hg rename ../d1/d11 d3)
  moving ../d1/d11/a1 to d3/d11/a1 (glob)
  $ hg status -C
  A d2/d3/d11/a1
    d1/d11/a1
  R d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d3

move --after a directory using a relative path

  $ (cd d2; mkdir d3; mv ../d1/d11 d3; hg rename --after ../d1/d11 d3)
  moving ../d1/d11/a1 to d3/d11/a1 (glob)
  $ hg status -C
  A d2/d3/d11/a1
    d1/d11/a1
  R d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d3

move directory d1/d11 to an existing directory d2 (removes empty d1)

  $ hg rename d1/d11/ d2
  moving d1/d11/a1 to d2/d11/a1 (glob)
  $ hg status -C
  A d2/d11/a1
    d1/d11/a1
  R d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d11

move directories d1 and d2 to a new directory d3

  $ mkdir d3
  $ hg rename d1 d2 d3
  moving d1/a to d3/d1/a (glob)
  moving d1/b to d3/d1/b (glob)
  moving d1/ba to d3/d1/ba (glob)
  moving d1/d11/a1 to d3/d1/d11/a1 (glob)
  moving d2/b to d3/d2/b (glob)
  $ hg status -C
  A d3/d1/a
    d1/a
  A d3/d1/b
    d1/b
  A d3/d1/ba
    d1/ba
  A d3/d1/d11/a1
    d1/d11/a1
  A d3/d2/b
    d2/b
  R d1/a
  R d1/b
  R d1/ba
  R d1/d11/a1
  R d2/b
  $ hg update -C
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move --after directories d1 and d2 to a new directory d3

  $ mkdir d3
  $ mv d1 d2 d3
  $ hg rename --after d1 d2 d3
  moving d1/a to d3/d1/a (glob)
  moving d1/b to d3/d1/b (glob)
  moving d1/ba to d3/d1/ba (glob)
  moving d1/d11/a1 to d3/d1/d11/a1 (glob)
  moving d2/b to d3/d2/b (glob)
  $ hg status -C
  A d3/d1/a
    d1/a
  A d3/d1/b
    d1/b
  A d3/d1/ba
    d1/ba
  A d3/d1/d11/a1
    d1/d11/a1
  A d3/d2/b
    d2/b
  R d1/a
  R d1/b
  R d1/ba
  R d1/d11/a1
  R d2/b
  $ hg update -C
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move everything under directory d1 to existing directory d2, do not
overwrite existing files (d2/b)

  $ hg rename d1/* d2
  d2/b: not overwriting - file exists
  moving d1/d11/a1 to d2/d11/a1 (glob)
  $ hg status -C
  A d2/a
    d1/a
  A d2/ba
    d1/ba
  A d2/d11/a1
    d1/d11/a1
  R d1/a
  R d1/ba
  R d1/d11/a1
  $ diff -u d1/b d2/b
  --- d1/b	* (glob)
  +++ d2/b	* (glob)
  @@ * (glob)
  -d1/b
  +d2/b
  [1]
  $ hg update -C
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/a d2/ba d2/d11/a1

attempt to move one file into a non-existent directory

  $ hg rename d1/a dx/
  abort: destination dx/ is not a directory
  [255]
  $ hg status -C
  $ hg update -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

attempt to move potentially more than one file into a non-existent directory

  $ hg rename 'glob:d1/**' dx
  abort: with multiple sources, destination must be an existing directory
  [255]

move every file under d1 to d2/d21 (glob)

  $ mkdir d2/d21
  $ hg rename 'glob:d1/**' d2/d21
  moving d1/a to d2/d21/a (glob)
  moving d1/b to d2/d21/b (glob)
  moving d1/ba to d2/d21/ba (glob)
  moving d1/d11/a1 to d2/d21/a1 (glob)
  $ hg status -C
  A d2/d21/a
    d1/a
  A d2/d21/a1
    d1/d11/a1
  A d2/d21/b
    d1/b
  A d2/d21/ba
    d1/ba
  R d1/a
  R d1/b
  R d1/ba
  R d1/d11/a1
  $ hg update -C
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d21

move --after some files under d1 to d2/d21 (glob)

  $ mkdir d2/d21
  $ mv d1/a d1/d11/a1 d2/d21
  $ hg rename --after 'glob:d1/**' d2/d21
  moving d1/a to d2/d21/a (glob)
  d1/b: not recording move - d2/d21/b does not exist (glob)
  d1/ba: not recording move - d2/d21/ba does not exist (glob)
  moving d1/d11/a1 to d2/d21/a1 (glob)
  $ hg status -C
  A d2/d21/a
    d1/a
  A d2/d21/a1
    d1/d11/a1
  R d1/a
  R d1/d11/a1
  $ hg update -C
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d21

move every file under d1 starting with an 'a' to d2/d21 (regexp)

  $ mkdir d2/d21
  $ hg rename 're:d1/([^a][^/]*/)*a.*' d2/d21
  moving d1/a to d2/d21/a (glob)
  moving d1/d11/a1 to d2/d21/a1 (glob)
  $ hg status -C
  A d2/d21/a
    d1/a
  A d2/d21/a1
    d1/d11/a1
  R d1/a
  R d1/d11/a1
  $ hg update -C
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d21

attempt to overwrite an existing file

  $ echo "ca" > d1/ca
  $ hg rename d1/ba d1/ca
  d1/ca: not overwriting - file exists
  $ hg status -C
  ? d1/ca
  $ hg update -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

forced overwrite of an existing file

  $ echo "ca" > d1/ca
  $ hg rename --force d1/ba d1/ca
  $ hg status -C
  A d1/ca
    d1/ba
  R d1/ba
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/ca

attempt to overwrite an existing broken symlink

#if symlink
  $ ln -s ba d1/ca
  $ hg rename --traceback d1/ba d1/ca
  d1/ca: not overwriting - file exists
  $ hg status -C
  ? d1/ca
  $ hg update -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/ca

replace a symlink with a file

  $ ln -s ba d1/ca
  $ hg rename --force d1/ba d1/ca
  $ hg status -C
  A d1/ca
    d1/ba
  R d1/ba
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/ca
#endif

do not copy more than one source file to the same destination file

  $ mkdir d3
  $ hg rename d1/* d2/* d3
  moving d1/d11/a1 to d3/d11/a1 (glob)
  d3/b: not overwriting - d2/b collides with d1/b
  $ hg status -C
  A d3/a
    d1/a
  A d3/b
    d1/b
  A d3/ba
    d1/ba
  A d3/d11/a1
    d1/d11/a1
  R d1/a
  R d1/b
  R d1/ba
  R d1/d11/a1
  $ hg update -C
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move a whole subtree with "hg rename ."

  $ mkdir d3
  $ (cd d1; hg rename . ../d3)
  moving a to ../d3/d1/a
  moving b to ../d3/d1/b
  moving ba to ../d3/d1/ba
  moving d11/a1 to ../d3/d1/d11/a1 (glob)
  $ hg status -C
  A d3/d1/a
    d1/a
  A d3/d1/b
    d1/b
  A d3/d1/ba
    d1/ba
  A d3/d1/d11/a1
    d1/d11/a1
  R d1/a
  R d1/b
  R d1/ba
  R d1/d11/a1
  $ hg update -C
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move a whole subtree with "hg rename --after ."

  $ mkdir d3
  $ mv d1/* d3
  $ (cd d1; hg rename --after . ../d3)
  moving a to ../d3/a
  moving b to ../d3/b
  moving ba to ../d3/ba
  moving d11/a1 to ../d3/d11/a1 (glob)
  $ hg status -C
  A d3/a
    d1/a
  A d3/b
    d1/b
  A d3/ba
    d1/ba
  A d3/d11/a1
    d1/d11/a1
  R d1/a
  R d1/b
  R d1/ba
  R d1/d11/a1
  $ hg update -C
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move the parent tree with "hg rename .."

  $ (cd d1/d11; hg rename .. ../../d3)
  moving ../a to ../../d3/a (glob)
  moving ../b to ../../d3/b (glob)
  moving ../ba to ../../d3/ba (glob)
  moving a1 to ../../d3/d11/a1
  $ hg status -C
  A d3/a
    d1/a
  A d3/b
    d1/b
  A d3/ba
    d1/ba
  A d3/d11/a1
    d1/d11/a1
  R d1/a
  R d1/b
  R d1/ba
  R d1/d11/a1
  $ hg update -C
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

skip removed files

  $ hg remove d1/b
  $ hg rename d1 d3
  moving d1/a to d3/a (glob)
  moving d1/ba to d3/ba (glob)
  moving d1/d11/a1 to d3/d11/a1 (glob)
  $ hg status -C
  A d3/a
    d1/a
  A d3/ba
    d1/ba
  A d3/d11/a1
    d1/d11/a1
  R d1/a
  R d1/b
  R d1/ba
  R d1/d11/a1
  $ hg update -C
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

transitive rename

  $ hg rename d1/b d1/bb
  $ hg rename d1/bb d1/bc
  $ hg status -C
  A d1/bc
    d1/b
  R d1/b
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/bc

transitive rename --after

  $ hg rename d1/b d1/bb
  $ mv d1/bb d1/bc
  $ hg rename --after d1/bb d1/bc
  $ hg status -C
  A d1/bc
    d1/b
  R d1/b
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/bc

  $ echo "# idempotent renames (d1/b -> d1/bb followed by d1/bb -> d1/b)"
  # idempotent renames (d1/b -> d1/bb followed by d1/bb -> d1/b)
  $ hg rename d1/b d1/bb
  $ echo "some stuff added to d1/bb" >> d1/bb
  $ hg rename d1/bb d1/b
  $ hg status -C
  M d1/b
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

overwriting with renames (issue1959)

  $ hg rename d1/a d1/c
  $ hg rename d1/b d1/a
  $ hg status -C
  M d1/a
    d1/b
  A d1/c
    d1/a
  R d1/b
  $ hg diff --git
  diff --git a/d1/a b/d1/a
  --- a/d1/a
  +++ b/d1/a
  @@ -1,1 +1,1 @@
  -d1/a
  +d1/b
  diff --git a/d1/b b/d1/b
  deleted file mode 100644
  --- a/d1/b
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -d1/b
  diff --git a/d1/a b/d1/c
  copy from d1/a
  copy to d1/c
  $ hg update -C
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/c # The file was marked as added, so 'hg update' action  was 'forget'

check illegal path components

  $ hg rename d1/d11/a1 .hg/foo
  abort: path contains illegal component: .hg/foo (glob)
  [255]
  $ hg status -C
  $ hg rename d1/d11/a1 ../foo
  abort: ../foo not under root '$TESTTMP'
  [255]
  $ hg status -C

  $ mv d1/d11/a1 .hg/foo
  $ hg rename --after d1/d11/a1 .hg/foo
  abort: path contains illegal component: .hg/foo (glob)
  [255]
  $ hg status -C
  ! d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm .hg/foo

  $ hg rename d1/d11/a1 .hg
  abort: path contains illegal component: .hg/a1 (glob)
  [255]
  $ hg --config extensions.largefiles= rename d1/d11/a1 .hg
  abort: path contains illegal component: .hg/a1 (glob)
  [255]
  $ hg status -C
  $ hg rename d1/d11/a1 ..
  abort: ../a1 not under root '$TESTTMP' (glob)
  [255]
  $ hg --config extensions.largefiles= rename d1/d11/a1 ..
  abort: ../a1 not under root '$TESTTMP' (glob)
  [255]
  $ hg status -C

  $ mv d1/d11/a1 .hg
  $ hg rename --after d1/d11/a1 .hg
  abort: path contains illegal component: .hg/a1 (glob)
  [255]
  $ hg status -C
  ! d1/d11/a1
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm .hg/a1

  $ (cd d1/d11; hg rename ../../d2/b ../../.hg/foo)
  abort: path contains illegal component: .hg/foo (glob)
  [255]
  $ hg status -C
  $ (cd d1/d11; hg rename ../../d2/b ../../../foo)
  abort: ../../../foo not under root '$TESTTMP'
  [255]
  $ hg status -C

