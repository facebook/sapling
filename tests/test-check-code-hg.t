#require test-repo

  $ check_code="$RUNTESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ hg locate | sed 's-\\-/-g' |
  >   xargs "$check_code" --warnings --per-file=0 || false
  Skipping fastmanifest/CMakeLists.txt it has no-che?k-code (glob)
  Skipping fastmanifest/bsearch.c it has no-che?k-code (glob)
  Skipping fastmanifest/bsearch.h it has no-che?k-code (glob)
  Skipping fastmanifest/bsearch_test.c it has no-che?k-code (glob)
  Skipping fastmanifest/buffer.c it has no-che?k-code (glob)
  Skipping fastmanifest/buffer.h it has no-che?k-code (glob)
  Skipping fastmanifest/checksum.c it has no-che?k-code (glob)
  Skipping fastmanifest/checksum.h it has no-che?k-code (glob)
  Skipping fastmanifest/checksum_test.c it has no-che?k-code (glob)
  Skipping fastmanifest/convert.h it has no-che?k-code (glob)
  Skipping fastmanifest/internal_result.h it has no-che?k-code (glob)
  Skipping fastmanifest/node.c it has no-che?k-code (glob)
  Skipping fastmanifest/node.h it has no-che?k-code (glob)
  Skipping fastmanifest/node_test.c it has no-che?k-code (glob)
  Skipping fastmanifest/null_test.c it has no-che?k-code (glob)
  Skipping fastmanifest/result.h it has no-che?k-code (glob)
  Skipping fastmanifest/tests.c it has no-che?k-code (glob)
  Skipping fastmanifest/tests.h it has no-che?k-code (glob)
  Skipping fastmanifest/tree.c it has no-che?k-code (glob)
  Skipping fastmanifest/tree.h it has no-che?k-code (glob)
  Skipping fastmanifest/tree_arena.c it has no-che?k-code (glob)
  Skipping fastmanifest/tree_arena.h it has no-che?k-code (glob)
  Skipping fastmanifest/tree_convert.c it has no-che?k-code (glob)
  Skipping fastmanifest/tree_convert_rt.c it has no-che?k-code (glob)
  Skipping fastmanifest/tree_convert_test.c it has no-che?k-code (glob)
  Skipping fastmanifest/tree_copy.c it has no-che?k-code (glob)
  Skipping fastmanifest/tree_copy_test.c it has no-che?k-code (glob)
  Skipping fastmanifest/tree_path.c it has no-che?k-code (glob)
  Skipping fastmanifest/tree_path.h it has no-che?k-code (glob)
  Skipping fastmanifest/tree_test.c it has no-che?k-code (glob)
  Skipping fastmanifest_wrapper.c it has no-che?k-code (glob)
  Skipping statprof.py it has no-che?k-code (glob)
