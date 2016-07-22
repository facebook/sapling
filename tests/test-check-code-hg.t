#require test-repo

  $ check_code="$RUNTESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ hg locate | sed 's-\\-/-g' |
  >   xargs "$check_code" --warnings --per-file=0 || false
  Skipping cfastmanifest.c it has no-che?k-code (glob)
  Skipping cfastmanifest/CMakeLists.txt it has no-che?k-code (glob)
  Skipping cfastmanifest/bsearch.c it has no-che?k-code (glob)
  Skipping cfastmanifest/bsearch.h it has no-che?k-code (glob)
  Skipping cfastmanifest/bsearch_test.c it has no-che?k-code (glob)
  Skipping cfastmanifest/buffer.c it has no-che?k-code (glob)
  Skipping cfastmanifest/buffer.h it has no-che?k-code (glob)
  Skipping cfastmanifest/checksum.c it has no-che?k-code (glob)
  Skipping cfastmanifest/checksum.h it has no-che?k-code (glob)
  Skipping cfastmanifest/checksum_test.c it has no-che?k-code (glob)
  Skipping cfastmanifest/convert.h it has no-che?k-code (glob)
  Skipping cfastmanifest/internal_result.h it has no-che?k-code (glob)
  Skipping cfastmanifest/node.c it has no-che?k-code (glob)
  Skipping cfastmanifest/node.h it has no-che?k-code (glob)
  Skipping cfastmanifest/node_test.c it has no-che?k-code (glob)
  Skipping cfastmanifest/null_test.c it has no-che?k-code (glob)
  Skipping cfastmanifest/result.h it has no-che?k-code (glob)
  Skipping cfastmanifest/tests.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tests.h it has no-che?k-code (glob)
  Skipping cfastmanifest/tree.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree.h it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_arena.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_arena.h it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_convert.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_convert_rt.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_convert_test.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_copy.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_copy_test.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_diff.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_diff_test.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_disk.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_disk_test.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_iterate_rt.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_iterator.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_iterator.h it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_iterator_test.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_path.c it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_path.h it has no-che?k-code (glob)
  Skipping cfastmanifest/tree_test.c it has no-che?k-code (glob)
  Skipping statprof.py it has no-che?k-code (glob)

Check foreign extensions are only used after checks

  $ hg locate 'test-*.t' | xargs $TESTDIR/check-foreignext.py
