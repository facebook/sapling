# Copyright 2016 Facebook Inc.
#
# Locate and load arcanist configuration for a project

import errno
import json
import os
from mercurial import (
    cmdutil,
    error
)

cmdtable = {}
command = cmdutil.command(cmdtable)

class ArcConfigError(Exception):
    pass

def _load_file(filename):
    try:
        with open(filename, 'r') as f:
            return json.loads(f.read())
    except IOError as ex:
        if ex.errno == errno.ENOENT:
            return None
        raise

def load_for_path(path):
    homedir = os.getenv('HOME')
    if not homedir:
        raise ArcConfigError('$HOME environment variable not found')

    # Use their own file as a basis
    userconfig = _load_file(os.path.join(homedir, '.arcrc')) or {}

    # Walk up the path and augment with an .arcconfig if we find it,
    # terminating the search at that point.
    path = os.path.abspath(path)
    while len(path) > 1:
        config = _load_file(os.path.join(path, '.arcconfig'))
        if config is not None:
            userconfig.update(config)
            # Return the located path too, as we need this for figuring
            # out where we are relative to the fbsource root.
            userconfig['_arcconfig_path'] = path
            return userconfig
        path = os.path.dirname(path)

    raise ArcConfigError('no .arcconfig found')

@command('debugarcconfig')
def debugarcconfig(ui, repo, *args, **opts):
    """ exists purely for testing and diagnostic purposes """
    try:
        config = load_for_path(repo.root)
        ui.write(json.dumps(config, sort_keys=True), '\n')
    except ArcConfigError as ex:
        raise error.Abort(str(ex))
