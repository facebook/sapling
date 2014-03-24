""" Layout detection for subversion repos.

Figure out what layout we should be using, based on config, command
line flags, subversion contents, and anything else we decide to base
it on.

"""

import os.path

from mercurial import util as hgutil

import __init__ as layouts

def layout_from_subversion(svn, revision=None, meta=None):
    """ Guess what layout to use based on directories under the svn root.

    This is intended for use during bootstrapping.  It guesses which
    layout to use based on the presence or absence of the conventional
    trunk, branches, tags dirs immediately under the path your are
    cloning.

    Additionally, this will write the layout in use to the ui object
    passed, if any.

    """
    # import late to avoid trouble when running the test suite
    try:
        from hgext_hgsubversion import svnwrap
    except ImportError:
        from hgsubversion import svnwrap

    try:
        rootlist = svn.list_dir('', revision=revision)
    except svnwrap.SubversionException, e:
        err = "%s (subversion error: %d)" % (e.args[0], e.args[1])
        raise hgutil.Abort(err)
    if sum(map(lambda x: x in rootlist, ('branches', 'tags', 'trunk'))):
        layout = 'standard'
    else:
        layout = 'single'
    meta.ui.setconfig('hgsubversion', 'layout', layout)
    return layout

def layout_from_config(meta, allow_auto=False):
    """ Load the layout we are using based on config

    We will read the config from the ui object.  Pass allow_auto=True
    if you are doing bootstrapping and can detect the layout in
    another manner if you get auto.  Otherwise, we will abort if we
    detect the layout as auto.
    """

    layout = meta.ui.config('hgsubversion', 'layout', default='auto')
    if layout == 'auto' and not allow_auto:
        raise hgutil.Abort('layout not yet determined')
    elif layout not in layouts.NAME_TO_CLASS and layout != 'auto':
        raise hgutil.Abort("unknown layout '%s'" % layout)
    return layout

def layout_from_file(meta):
    """ Load the layout in use from the metadata file.
    """

    # import late to avoid trouble when running the test suite
    try:
        from hgext_hgsubversion import util
    except ImportError:
        from hgsubversion import util

    layout = util.load(meta.layout_file)
    if layout:
        meta.ui.setconfig('hgsubversion', 'layout', layout)
    return layout

def layout_from_commit(subdir, revpath, branch, meta):
    """ Guess what the layout is based existing commit info

    Specifically, this compares the subdir for the repository and the
    revpath as extracted from the convinfo in the commit.  If they
    match, the layout is assumed to be single.  Otherwise, it tries
    the available layouts and selects the first one that would
    translate the given branch to the given revpath.

    """

    subdir = subdir or '/'
    if subdir == revpath:
        return 'single'

    candidates = set()
    for layout in layouts.NAME_TO_CLASS:
        layoutobj = layouts.layout_from_name(layout, meta)
        try:
            remotepath = layoutobj.remotepath(branch, subdir)
        except KeyError:
            continue
        if  remotepath == revpath:
            candidates.add(layout)

    if len(candidates) == 1:
        return candidates.pop()
    elif candidates:
        config_layout = layout_from_config(meta, allow_auto=True)
        if config_layout in candidates:
            return config_layout

    return 'standard'
