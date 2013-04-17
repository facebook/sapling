""" Layout detection for subversion repos.

Figure out what layout we should be using, based on config, command
line flags, subversion contents, and anything else we decide to base
it on.

"""

import os.path

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

def layout_from_config(ui, allow_auto=False):
    """ Load the layout we are using based on config

    We will read the config from the ui object.  Pass allow_auto=True
    if you are doing bootstrapping and can detect the layout in
    another manner if you get auto.  Otherwise, we will abort if we
    detect the layout as auto.
    """

    layout = ui.config('hgsubversion', 'layout', default='auto')
    if layout == 'auto' and not allow_auto:
        raise hgutil.Abort('layout not yet determined')
    elif layout not in ('auto', 'single', 'standard'):
        raise hgutil.Abort("unknown layout '%s'" % layout)
    return layout

def layout_from_file(meta_data_dir, ui=None):
    """ Load the layout in use from the metadata file.

    If you pass the ui arg, we will also write the layout to the
    config for that ui.

    """

    layout = None
    layoutfile = os.path.join(meta_data_dir, 'layout')
    if os.path.exists(layoutfile):
        f = open(layoutfile)
        layout = f.read().strip()
        f.close()
        if ui:
            ui.setconfig('hgsubversion', 'layout', layout)
    return layout

def layout_from_commit(subdir, revpath):
    """ Guess what the layout is based existing commit info

    Specifically, this compares the subdir for the repository and the
    revpath as extracted from the convinfo in the commit.

    """

    if (subdir or '/') == revpath:
        layout = 'single'
    else:
        layout = 'standard'

    return layout
