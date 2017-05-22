# Copyright 2016 Facebook Inc.
#
# Locate and load arcanist configuration for a project

import errno
import json
import os
from mercurial import (
    error,
    registrar,
)

cmdtable = {}
command = registrar.command(cmdtable)

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
    # location where `arc install-certificate` writes .arcrc
    if os.name == 'nt':
        envvar = 'APPDATA'
    else:
        envvar = 'HOME'
    homedir = os.getenv(envvar)
    if not homedir:
        raise ArcConfigError('$%s environment variable not found' % envvar)

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
