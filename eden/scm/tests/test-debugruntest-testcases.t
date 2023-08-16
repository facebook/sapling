#debugruntest-compatible

#testcases case1 case2

  >>> pydoc_var_for_both = "hello"

    lexical_var_for_both = "hello"

#if case1
  $ echo in case1
  in case1

    >>> var_for_only_case1 = pydoc_var_for_both

  >>> var_for_only_case1
  'hello'
  >>> pydoc_var_for_both
  'hello'

    pydoc_var_for_both
    lexical_var_for_both
#else
  $ echo not case1
  not case1
#endif

#if case2
  $ echo in case2
  in case2

Make sure we can't see case1's variable.
  >>> var_for_only_case1
  name 'var_for_only_case1' is not defined
  >>> pydoc_var_for_both
  'hello'

    pydoc_var_for_both
    lexical_var_for_both
#endif

  $ echo in shared test
  in shared test

  $ hg init
