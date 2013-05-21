"""Module to hold the base API for layout classes.

This module should not contain any implementation, just a definition
of the API concrete layouts are expected to implement.

"""

from mercurial import util as hgutil

class BaseLayout(object):

    def __init__(self, ui):
        self.ui = ui

    def __unimplemented(self, method_name):
        raise NotImplementedError(
            "Incomplete layout implementation: %s.%s doesn't implement %s" %
            (self.__module__, self.__name__, method_name))

    def localname(self, path):
        """Compute the local name for a branch located at path.

        path should be relative to the repo url.

        """
        self.__unimplemented('localname')

    def remotename(self, branch):
        """Compute a subversion path for a mercurial branch name

        This should return a path relative to the repo url

        """
        self.__unimplemented('remotename')

    def remotepath(self, branch, subdir='/'):
        """Compute a  subversion path for a mercurial branch name.

        This should return an absolute path, assuming our repo root is at subdir
        A false subdir shall be taken to mean /.

        """
        self.__unimplemented('remotepath')
