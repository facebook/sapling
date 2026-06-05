#chg-compatible
#require no-eden

  $ configure modernclient
  $ newclientrepo repo
  $ mkdir d1 d1/d11 d2
  $ echo d1/a > d1/a
  $ echo d1/ba > d1/ba
  $ echo d1/a1 > d1/d11/a1
  $ echo d1/b > d1/b
  $ echo d2/b > d2/b
  $ sl add d1/a d1/b d1/ba d1/d11/a1 d2/b
  $ sl commit -m "1"

rename a single file

  $ sl rename d1/d11/a1 d2/c
  $ sl --config ui.portablefilenames=abort rename d1/a d1/con.xml
  abort: filename contains 'con', which is reserved on Windows: d1/con.xml
  [255]
  $ sl sum
  parent: * (glob)
   1
  commit: 1 renamed
  phases: 1 draft
  $ sl status -C
  A d2/c
    d1/d11/a1
  R d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/c

rename a single file using absolute paths

  $ sl rename `pwd`/d1/d11/a1 `pwd`/d2/c
  $ sl status -C
  A d2/c
    d1/d11/a1
  R d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/c

rename --mark a single file

  $ mv d1/d11/a1 d2/c
  $ sl rename --mark d1/d11/a1 d2/c
  $ sl status -C
  A d2/c
    d1/d11/a1
  R d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/c

rename --mark a single file when src and tgt already tracked

  $ mv d1/d11/a1 d2/c
  $ sl addremove -s 0
  removing d1/d11/a1
  adding d2/c
  $ sl rename --mark d1/d11/a1 d2/c
  $ sl status -C
  A d2/c
    d1/d11/a1
  R d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/c

rename --mark a single file to a nonexistent target filename

  $ sl rename --mark d1/a dummy
  d1/a: not recording move - dummy does not exist

move a single file to an existing directory

  $ sl rename d1/d11/a1 d2
  $ sl status -C
  A d2/a1
    d1/d11/a1
  R d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/a1

move --mark a single file to an existing directory

  $ mv d1/d11/a1 d2
  $ sl rename --mark d1/d11/a1 d2
  $ sl status -C
  A d2/a1
    d1/d11/a1
  R d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/a1

rename a file using a relative path

  $ (cd d1/d11; sl rename ../../d2/b e)
  $ sl status -C
  A d1/d11/e
    d2/b
  R d2/b
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/d11/e

rename --mark a file using a relative path

  $ (cd d1/d11; mv ../../d2/b e; sl rename --mark ../../d2/b e)
  $ sl status -C
  A d1/d11/e
    d2/b
  R d2/b
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/d11/e

rename directory d1 as d3

  $ sl rename d1/ d3
  moving d1/a to d3/a
  moving d1/b to d3/b
  moving d1/ba to d3/ba
  moving d1/d11/a1 to d3/d11/a1
  $ sl status -C
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
  $ sl goto -C tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

rename --mark directory d1 as d3

  $ mv d1 d3
  $ sl rename --mark d1 d3
  moving d1/a to d3/a
  moving d1/b to d3/b
  moving d1/ba to d3/ba
  moving d1/d11/a1 to d3/d11/a1
  $ sl status -C
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
  $ sl goto -C tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move a directory using a relative path

  $ (cd d2; mkdir d3; sl rename ../d1/d11 d3)
  moving ../d1/d11/a1 to d3/d11/a1
  $ sl status -C
  A d2/d3/d11/a1
    d1/d11/a1
  R d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d3

move --mark a directory using a relative path

  $ (cd d2; mkdir d3; mv ../d1/d11 d3; sl rename --mark ../d1/d11 d3)
  moving ../d1/d11/a1 to d3/d11/a1
  $ sl status -C
  A d2/d3/d11/a1
    d1/d11/a1
  R d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d3

move directory d1/d11 to an existing directory d2 (removes empty d1)

  $ sl rename d1/d11/ d2
  moving d1/d11/a1 to d2/d11/a1
  $ sl status -C
  A d2/d11/a1
    d1/d11/a1
  R d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d11

move directories d1 and d2 to a new directory d3

  $ mkdir d3
  $ sl rename d1 d2 d3
  moving d1/a to d3/d1/a
  moving d1/b to d3/d1/b
  moving d1/ba to d3/d1/ba
  moving d1/d11/a1 to d3/d1/d11/a1
  moving d2/b to d3/d2/b
  $ sl status -C
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
  $ sl goto -C tip
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move --mark directories d1 and d2 to a new directory d3

  $ mkdir d3
  $ mv d1 d2 d3
  $ sl rename --mark d1 d2 d3
  moving d1/a to d3/d1/a
  moving d1/b to d3/d1/b
  moving d1/ba to d3/d1/ba
  moving d1/d11/a1 to d3/d1/d11/a1
  moving d2/b to d3/d2/b
  $ sl status -C
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
  $ sl goto -C tip
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move everything under directory d1 to existing directory d2, do not
overwrite existing files (d2/b)

  $ sl rename d1/* d2
  d2/b: not overwriting - file already committed
  (use 'sl rename --amend --mark' to amend the current commit)
  moving d1/d11/a1 to d2/d11/a1
  $ sl status -C
  A d2/a
    d1/a
  A d2/ba
    d1/ba
  A d2/d11/a1
    d1/d11/a1
  R d1/a
  R d1/ba
  R d1/d11/a1
  $ cat d1/b d2/b
  d1/b
  d2/b
  $ sl goto -C tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d2/a d2/ba d2/d11/a1

attempt to move one file into a non-existent directory

  $ sl rename d1/a dx/
  abort: destination dx/ is not a directory
  [255]
  $ sl status -C
  $ sl goto -C tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

attempt to move potentially more than one file into a non-existent directory

  $ sl rename 'glob:d1/**' dx
  abort: with multiple sources, destination must be an existing directory
  [255]

move every file under d1 to d2/d21

  $ mkdir d2/d21
  $ sl rename 'glob:d1/**' d2/d21
  moving d1/a to d2/d21/a
  moving d1/b to d2/d21/b
  moving d1/ba to d2/d21/ba
  moving d1/d11/a1 to d2/d21/a1
  $ sl status -C
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
  $ sl goto -C tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d21

move --mark some files under d1 to d2/d21

  $ mkdir d2/d21
  $ mv d1/a d1/d11/a1 d2/d21
  $ sl rename --mark 'glob:d1/**' d2/d21
  moving d1/a to d2/d21/a
  d1/b: not recording move - d2/d21/b does not exist
  d1/ba: not recording move - d2/d21/ba does not exist
  moving d1/d11/a1 to d2/d21/a1
  $ sl status -C
  A d2/d21/a
    d1/a
  A d2/d21/a1
    d1/d11/a1
  R d1/a
  R d1/d11/a1
  $ sl goto -C tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d21

move every file under d1 starting with an 'a' to d2/d21 (regexp)

  $ mkdir d2/d21
  $ sl rename 're:d1/([^a][^/]*/)*a.*' d2/d21
  moving d1/a to d2/d21/a
  moving d1/d11/a1 to d2/d21/a1
  $ sl status -C
  A d2/d21/a
    d1/a
  A d2/d21/a1
    d1/d11/a1
  R d1/a
  R d1/d11/a1
  $ sl goto -C tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d2/d21

attempt to overwrite an existing file

  $ echo "ca" > d1/ca
  $ sl rename d1/ba d1/ca
  d1/ca: not overwriting - file exists
  (sl rename --mark to record the rename)
  $ sl status -C
  ? d1/ca
  $ sl goto -C tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

forced overwrite of an existing file

  $ echo "ca" > d1/ca
  $ sl rename --force d1/ba d1/ca
  $ sl status -C
  A d1/ca
    d1/ba
  R d1/ba
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/ca

attempt to overwrite an existing broken symlink

#if symlink
  $ ln -s ba d1/ca
  $ sl rename --traceback d1/ba d1/ca
  d1/ca: not overwriting - file exists
  (sl rename --mark to record the rename)
  $ sl status -C
  ? d1/ca
  $ sl goto -C tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/ca

replace a symlink with a file

  $ ln -s ba d1/ca
  $ sl rename --force d1/ba d1/ca
  $ sl status -C
  A d1/ca
    d1/ba
  R d1/ba
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/ca
#endif

do not copy more than one source file to the same destination file

  $ mkdir d3
  $ sl rename d1/* d2/* d3
  moving d1/d11/a1 to d3/d11/a1
  d3/b: not overwriting - d2/b collides with d1/b
  $ sl status -C
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
  $ sl goto -C tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move a whole subtree with "sl rename ."

  $ mkdir d3
  $ (cd d1; sl rename . ../d3)
  moving a to ../d3/d1/a
  moving b to ../d3/d1/b
  moving ba to ../d3/d1/ba
  moving d11/a1 to ../d3/d1/d11/a1
  $ sl status -C
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
  $ sl goto -C tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move a whole subtree with "sl rename --mark ."

  $ mkdir d3
  $ mv d1/* d3
  $ (cd d1; sl rename --mark . ../d3)
  moving a to ../d3/a
  moving b to ../d3/b
  moving ba to ../d3/ba
  moving d11/a1 to ../d3/d11/a1
  $ sl status -C
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
  $ sl goto -C tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

move the parent tree with "sl rename .."

  $ (cd d1/d11; sl rename .. ../../d3)
  moving ../a to ../../d3/a
  moving ../b to ../../d3/b
  moving ../ba to ../../d3/ba
  moving a1 to ../../d3/d11/a1
  $ sl status -C
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
  $ sl goto -C tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

skip removed files

  $ sl remove d1/b
  $ sl rename d1 d3
  moving d1/a to d3/a
  moving d1/ba to d3/ba
  moving d1/d11/a1 to d3/d11/a1
  $ sl status -C
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
  $ sl goto -C tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf d3

transitive rename

  $ sl rename d1/b d1/bb
  $ sl rename d1/bb d1/bc
  $ sl status -C
  A d1/bc
    d1/b
  R d1/b
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/bc

transitive rename --mark

  $ sl rename d1/b d1/bb
  $ mv d1/bb d1/bc
  $ sl rename --mark d1/bb d1/bc
  $ sl status -C
  A d1/bc
    d1/b
  R d1/b
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/bc

  $ echo "# idempotent renames (d1/b -> d1/bb followed by d1/bb -> d1/b)"
  # idempotent renames (d1/b -> d1/bb followed by d1/bb -> d1/b)
  $ sl rename d1/b d1/bb
  $ echo "some stuff added to d1/bb" >> d1/bb
  $ sl rename d1/bb d1/b
  $ sl status -C
  M d1/b
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

overwriting with renames (issue1959)

  $ sl rename d1/a d1/c
  $ sl rename d1/b d1/a
  $ sl status -C
  M d1/a
    d1/b
  A d1/c
    d1/a
  R d1/b
  $ sl diff --git
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
  $ sl goto -C tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm d1/c # The file was marked as added, so 'sl goto' action  was 'forget'

check illegal path components

  $ sl rename d1/d11/a1 .sl/foo
  abort: path contains illegal component '.sl': .sl/foo
  [255]
  $ sl status -C
  $ sl rename d1/d11/a1 ../foo
  abort: cwd relative path '../foo' is not under root '$TESTTMP/repo'
  (hint: consider using --cwd to change working directory)
  [255]
  $ sl status -C

  $ mv d1/d11/a1 .sl/foo
  $ sl rename --mark d1/d11/a1 .sl/foo
  abort: path contains illegal component '.sl': .sl/foo
  [255]
  $ sl status -C
  ! d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm .sl/foo

  $ sl rename d1/d11/a1 .sl
  abort: path contains illegal component '.sl': .sl/a1
  [255]
  $ sl status -C
  $ sl rename d1/d11/a1 ..
  abort: cwd relative path '..' is not under root '$TESTTMP/repo'
  (hint: consider using --cwd to change working directory)
  [255]
  $ sl status -C

  $ mv d1/d11/a1 .sl
  $ sl rename --mark d1/d11/a1 .sl
  abort: path contains illegal component '.sl': .sl/a1
  [255]
  $ sl status -C
  ! d1/d11/a1
  $ sl goto -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm .sl/a1

  $ (cd d1/d11; sl rename ../../d2/b ../../.sl/foo)
  abort: path contains illegal component '.sl': .sl/foo
  [255]
  $ sl status -C
  $ (cd d1/d11; sl rename ../../d2/b ../../../foo)
  abort: cwd relative path '../../../foo' is not under root '$TESTTMP/repo'
  (hint: consider using --cwd to change working directory)
  [255]
  $ sl status -C
