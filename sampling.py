# sampling.py - sample collection extension
#
# Copyright 2016 Facebook
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# Usage:
# - This extension enhances ui.log(category, message, key=value, ...)
# to also append the logged events as JSON to a file.
# - The events are separated by NULL characters: '\0'.
# - The file is either specified with the HG_SAMPLING_FILEPATH environment
# variable or the sampling.filepath configuration.
# - If the file cannot be created or accessed, fails silently


import json, os

def _parentfolderexists(f):
    return (f is not None and
            os.path.exists(os.path.dirname(os.path.normpath(f))))

def _getcandidatelocation(ui):
    for candidatelocation in (os.environ.get("HG_SAMPLING_FILEPATH", None),
                              ui.config("sampling", "filepath", "")):
        if _parentfolderexists(candidatelocation):
            return candidatelocation
    return None

def uisetup(ui):
    class logtofile(ui.__class__):
        def log(self, event, *msg, **opts):
            """Redirect log event to a sampling file"""
            script = _getcandidatelocation(ui)
            if script:
                try:
                    with open(script, 'a') as outfile:
                        outfile.write(json.dumps({"event": event,
                                                  "msg": msg,
                                                  "opts": opts}))
                        outfile.write("\0")
                except EnvironmentError:
                    pass
            return super(logtofile, self).log(event, *msg, **opts)

    # Replace the class for this instance and all clones created from it:
    ui.__class__ = logtofile
