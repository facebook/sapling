#require no-eden

#testcases(*product(['case1', None], ['case2', None]))

#if case1

  >>> var_for_case1 = "hello case1"

#else

  >>> var_for_no_case1 = "bye case1"

#endif

#if case2

  >>> var_for_case2 = "hello case2"

#else

  >>> var_for_no_case2 = "bye case2"

#endif

#if case1 case2
  >>> var_for_case1
  'hello case1'

  >>> var_for_case2
  'hello case2'

  >>> var_for_no_case1
  name 'var_for_no_case1' is not defined

  >>> var_for_no_case2
  name 'var_for_no_case2' is not defined

#endif

#if case1 no-case2

  >>> var_for_case1
  'hello case1'

  >>> var_for_case2
  name 'var_for_case2' is not defined

  >>> var_for_no_case1
  name 'var_for_no_case1' is not defined

  >>> var_for_no_case2
  'bye case2'

#endif

#if no-case1 case2

  >>> var_for_case1
  name 'var_for_case1' is not defined

  >>> var_for_case2
  'hello case2'

  >>> var_for_no_case1
  'bye case1'

  >>> var_for_no_case2
  name 'var_for_no_case2' is not defined
#endif

#if no-case1 no-case2

  >>> var_for_case1
  name 'var_for_case1' is not defined

  >>> var_for_case2
  name 'var_for_case2' is not defined

  >>> var_for_no_case1
  'bye case1'

  >>> var_for_no_case2
  'bye case2'

#endif
