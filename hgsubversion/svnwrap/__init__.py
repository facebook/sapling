"""This is a special package because it contains (or will contain, as of now)
two parallel implementations of the same code. One implementation, the original,
uses the SWIG Python bindings. That's great, but those leak RAM and have a few
other quirks. The goal is to have this file automatically contain the "best"
available implementation without the user having to configure what is actually
present.
"""

from common import *

import os

choice = os.environ.get('HGSUBVERSION_BINDINGS', '').lower()

if choice == 'subvertpy':
    from subvertpy_wrapper import *
elif choice == 'swig':
    from svn_swig_wrapper import *
elif choice == 'none':
    # useful for verifying that demandimport works properly
    raise ImportError('cannot use hgsubversion; '
                      'bindings disabled using HGSUBVERSION_BINDINGS')
else:
    try:
        from subvertpy_wrapper import *
    except ImportError, e1:
        try:
            from svn_swig_wrapper import *
        except ImportError, e2:
            raise ImportError('no compatible bindings available:\n\n'
                              '%s\n%s\n\n'
                              'Please install either Subvertpy or the '
                              'Subversion Python SWIG bindings'
                              % (e2, e1))

del os, choice
