  $ cat > correct.py <<EOF
  > def toto(arg1, arg2):
  >     del arg2
  >     return (5 + 6, 9)
  > EOF
  $ cat > wrong.py <<EOF
  > def toto( arg1, arg2):
  >     del(arg2)
  >     return ( 5+6, 9)
  > EOF
  $ cat > quote.py <<EOF
  > # let's use quote in comments
  > (''' ( 4x5 )
  > but """\\''' and finally''',
  > """let's fool checkpatch""", '1+2',
  > '"""', 42+1, """and
  > ( 4-1 ) """, "( 1+1 )\" and ")
  > a, '\\\\\\\\', "\\\\\\" x-2", "c-1"
  > EOF
  $ cat > non-py24.py <<EOF
  > # Using builtins that does not exist in Python 2.4
  > if any():
  >     x = all()
  >     y = format(x)
  > 
  > # Do not complain about our own definition
  > def any(x):
  >     pass
  > EOF
  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ "$check_code" ./wrong.py ./correct.py ./quote.py ./non-py24.py
  ./wrong.py:1:
   > def toto( arg1, arg2):
   gratuitous whitespace in () or []
  ./wrong.py:2:
   >     del(arg2)
   del isn't a function
  ./wrong.py:3:
   >     return ( 5+6, 9)
   missing whitespace in expression
   gratuitous whitespace in () or []
  ./quote.py:5:
   > '"""', 42+1, """and
   missing whitespace in expression
  ./non-py24.py:2:
   > if any():
   any/all/format not available in Python 2.4
  ./non-py24.py:3:
   >     x = all()
   any/all/format not available in Python 2.4
  ./non-py24.py:4:
   >     y = format(x)
   any/all/format not available in Python 2.4
  [1]
