"""Module to hold the base API for layout classes.

This module should not contain any implementation, just a definition
of the API concrete layouts are expected to implement.

"""

from mercurial import util as hgutil

class BaseLayout(object):

    def __unimplemented(self, method_name):
        raise NotImplementedError(
            "Incomplete layout implementation: %s.%s doesn't implement %s" %
            (self.__module__, self.__name__, method_name))

    def localname(self, path):
        """Compute the local name for a branch located at path.
        """
        self.__unimplemented('localname')

    def remotename(self, branch):
        """Compute a subversion path for a mercurial branch name"""
        self.__unimplemented('remotename')
