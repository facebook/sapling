"""test line matching with some failing examples and some which warn

run-test.t only checks positive matches and can not see warnings
(both by design)
"""
from __future__ import print_function

import os, re
# this is hack to make sure no escape characters are inserted into the output
if 'TERM' in os.environ:
    del os.environ['TERM']
import doctest
run_tests = __import__('run-tests')

def prn(ex):
    m = ex.args[0]
    if isinstance(m, str):
        print(m)
    else:
        print(m.decode('utf-8'))

def lm(expected, output):
    r"""check if output matches expected

    does it generally work?
        >>> lm(b'H*e (glob)\n', b'Here\n')
        True

    fail on bad test data
        >>> try: lm(b'a\n',b'a')
        ... except AssertionError as ex: print(ex)
        missing newline
        >>> try: lm(b'single backslash\n', b'single \backslash\n')
        ... except AssertionError as ex: prn(ex)
        single backslash or unknown char
    """
    assert (expected.endswith(b'\n')
            and output.endswith(b'\n')), 'missing newline'
    assert not re.search(br'[^ \w\\/\r\n()*?]', expected + output), \
           b'single backslash or unknown char'
    match = run_tests.TTest.linematch(expected, output)
    if isinstance(match, str):
        return 'special: ' + match
    elif isinstance(match, bytes):
        return 'special: ' + match.decode('utf-8')
    else:
        return bool(match) # do not return match object

def wintests():
    r"""test matching like running on windows

    enable windows matching on any os
        >>> _osaltsep = os.altsep
        >>> os.altsep = True

    valid match on windows
        >>> lm(b'g/a*/d (glob)\n', b'g\\abc/d\n')
        True

    direct matching, glob unnecessary
        >>> lm(b'g/b (glob)\n', b'g/b\n')
        'special: -glob'

    missing glob
        >>> lm(b'/g/c/d/fg\n', b'\\g\\c\\d/fg\n')
        'special: +glob'

    restore os.altsep
        >>> os.altsep = _osaltsep
    """
    pass

def otherostests():
    r"""test matching like running on non-windows os

    disable windows matching on any os
        >>> _osaltsep = os.altsep
        >>> os.altsep = False

    backslash does not match slash
        >>> lm(b'h/a* (glob)\n', b'h\\ab\n')
        False

    direct matching glob can not be recognized
        >>> lm(b'h/b (glob)\n', b'h/b\n')
        True

    missing glob can not not be recognized
        >>> lm(b'/h/c/df/g/\n', b'\\h/c\\df/g\\\n')
        False

    restore os.altsep
        >>> os.altsep = _osaltsep
    """
    pass

if __name__ == '__main__':
    doctest.testmod()
