""" Layout detection for subversion repos.

Figure out what layout we should be using, based on config, command
line flags, subversion contents, and anything else we decide to base
it on.

"""

from mercurial import util as hgutil

import hgsubversion.svnwrap

def layout_from_subversion(svn, revision=None, ui=None):
    """ Guess what layout to use based on directories under the svn root.

    This is intended for use during bootstrapping.  It guesses which
    layout to use based on the presence or absence of the conventional
    trunk, branches, tags dirs immediately under the path your are
    cloning.

    Additionally, this will write the layout in use to the ui object
    passed, if any.

    """

    try:
        rootlist = svn.list_dir('', revision=revision)
    except svnwrap.SubversionException, e:
        err = "%s (subversion error: %d)" % (e.args[0], e.args[1])
        raise hgutil.Abort(err)
    if sum(map(lambda x: x in rootlist, ('branches', 'tags', 'trunk'))):
        layout = 'standard'
    else:
        layout = 'single'
    ui.setconfig('hgsubversion', 'layout', layout)
    return layout
