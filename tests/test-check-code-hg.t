  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..

  $ "$check_code" `hg manifest` | grep . && echo 'FAILURE IS NOT AN OPTION!!!'
  [1]

