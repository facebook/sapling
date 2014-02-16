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

        Implementations may indicate that no mapping is possible for
        the given branch by raising a KeyError.

        """
        self.__unimplemented('remotename')

    def remotepath(self, branch, subdir='/'):
        """Compute a  subversion path for a mercurial branch name.

        This should return an absolute path, assuming our repo root is at subdir
        A false subdir shall be taken to mean /.

        Implementations may indicate that no mapping is possible for
        the given branch by raising a KeyError.

        """
        self.__unimplemented('remotepath')

    def taglocations(self, metapath):
        """Return a list of locations within svn to search for tags

        Should be returned in reverse-sorted order.

        """
        self.__unimplemented('tagpaths')

    def get_path_tag(self, path, taglocations):
        """Get the tag name for the given svn path, if it is a possible tag.

        This function should return None if the path cannot be a tag.
        Returning a non-empty sring does not imply that the path is a
        tag, only that it is a candidate to be a tag.  Returning an
        empty string is an error.

        Path should be relative to the repo url.
        taglocations should be as returned by self.taglocations()

        """
        self.__unimplemented('get_path_tag')

    def split_remote_name(self, path, known_branches):
        """Split the path into a branch component and a local component.

        path should be relative to our repo url

        returns (branch_path, local_path)

        branch_path should be suitable to pass into localname,
        i.e. branch_path should NOT have a leading or trailing /

        local_path should be relative to the root of the Mercurial working dir

        Note that it is permissible to return a longer branch_path
        than is passed in iff the path that is passed in is a parent
        directory of exactly one branch.  This is intended to handle
        the case where we are importing a particular subdirectory of
        asubversion branch structure.

        """
        self.__unimplemented('split_remote_name')
