"""This is a special package because it contains (or will contain, as of now)
two parallel implementations of the same code. One implementation, the original,
uses the SWIG Python bindings. That's great, but those leak RAM and have a few
other quirks. There are new, up-and-coming ctypes bindings for Subversion which
look more promising, and are portible backwards to 1.4's libraries. The goal is
to have this file automatically contain the "best" available implementation
without the user having to configure what is actually present.
"""

#try:
#    # we do __import__ here so that the correct items get pulled in. Otherwise
#    # demandimport can make life difficult.
#    __import__('csvn')
#    from svn_ctypes_wrapper import *
#except ImportError, e:
from svn_swig_wrapper import *
