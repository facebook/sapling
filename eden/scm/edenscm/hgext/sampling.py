# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# sampling.py - sample collection extension
#
# Usage:
# - This extension enhances ui.log(category, message, key=value, ...)
# to also append filtered logged events as JSON to a file.
# - The events are separated by NULL characters: '\0'.
# - The file is either specified with the SCM_SAMPLING_FILEPATH environment
# variable or the sampling.filepath configuration.
# - If the file cannot be created or accessed, fails silently
#
# The configuration details can be found in the documentation of ui.log below
import json
import os
import weakref

from edenscm.mercurial import encoding, localrepo, pycompat, registrar, util


configtable = {}
configitem = registrar.configitem(configtable)

configitem("sampling", "filepath", default="")
configitem("sampling", "debug", default=False)


def _parentfolderexists(f):
    return f is not None and os.path.exists(os.path.dirname(os.path.normpath(f)))


def _getcandidatelocation(ui):
    for candidatelocation in (
        encoding.environ.get("SCM_SAMPLING_FILEPATH", None),
        ui.config("sampling", "filepath"),
    ):
        if _parentfolderexists(candidatelocation):
            return candidatelocation
    return None


def uisetup(ui):
    # pyre-fixme[11]: Annotation `__class__` is not defined as a type.
    class logtofile(ui.__class__):
        @classmethod
        def computesamplingfilters(cls, self):
            filtermap = {}
            for k in ui.configitems("sampling"):
                if not k[0].startswith("key."):
                    continue  # not a key
                filtermap[k[0][len("key.") :]] = k[1]
            return filtermap

        def log(self, event, *msg, **opts):
            """Redirect filtered log event to a sampling file
            The configuration looks like:
            [sampling]
            filepath = path/to/file
            key.eventname = value
            key.eventname2 = value2

            If an event name appears in the config, it is logged to the
            samplingfile augmented with value stored as ref.

            Example:
            [sampling]
            filepath = path/to/file
            key.perfstatus = perf_status

            Assuming that we call:
            ui.log('perfstatus', t=3)
            ui.log('perfcommit', t=3)
            ui.log('perfstatus', t=42)

            Then we will log in path/to/file, two JSON strings separated by \0
            one for each perfstatus, like:
            {"event":"perfstatus",
             "ref":"perf_status",
             "msg":"",
             "opts":{"t":3}}\0
            {"event":"perfstatus",
             "ref":"perf_status",
             "msg":"",
             "opts":{"t":42}}\0

            We will also log any given environmental vars to the env_vars log,
            if configured::

              [sampling]
              env_vars = PATH,SHELL
            """
            if not util.safehasattr(self, "samplingfilters"):
                self.samplingfilters = logtofile.computesamplingfilters(self)
            if event not in self.samplingfilters:
                return super(logtofile, self).log(event, *msg, **opts)

            # special case: remove less interesting blocked fields starting
            # with "unknown_" or "alias_".
            if event == "measuredtimes":
                opts = {
                    k: v
                    for k, v in opts.items()
                    if (not k.startswith("alias_") and not k.startswith("unknown_"))
                }

            ref = self.samplingfilters[event]
            script = _getcandidatelocation(ui)
            if script:
                debug = self.configbool("sampling", "debug")
                try:
                    opts["metrics_type"] = event
                    if msg and event != "metrics":
                        # do not keep message for "metrics", which only wants
                        # to log key/value dict.
                        if len(msg) == 1:
                            # don't try to format if there is only one item.
                            opts["msg"] = msg[0]
                        else:
                            # ui.log treats msg as a format string + format args.
                            try:
                                opts["msg"] = msg[0] % msg[1:]
                            except TypeError:
                                # formatting failed - just log each item of the
                                # message separately.
                                opts["msg"] = " ".join(msg)
                    with open(script, "a") as outfile:
                        outfile.write(json.dumps({"data": opts, "category": ref}))
                        outfile.write("\0")
                    if debug:
                        ui.write_err(
                            "%s\n" % json.dumps({"data": opts, "category": ref})
                        )
                except EnvironmentError:
                    pass
            return super(logtofile, self).log(event, *msg, **opts)

    # Replace the class for this instance and all clones created from it:
    ui.__class__ = logtofile


def getrelativecwd(repo):
    """Returns the current directory relative to the working copy root, or
    None if it's not in the working copy.
    """
    cwd = pycompat.getcwdsafe()
    if cwd.startswith(repo.root):
        return os.path.normpath(cwd[len(repo.root) + 1 :])
    else:
        return None


def gettopdir(repo):
    """Returns the first component of the current directory, if it's in the
     working copy.
     """
    reldir = getrelativecwd(repo)
    if reldir:
        components = reldir.split(pycompat.ossep)
        if len(components) > 0 and components[0] != ".":
            return components[0]
    else:
        return None


def telemetry(reporef):
    repo = reporef()
    if repo is None:
        return
    ui = repo.ui
    try:
        try:
            lfsmetrics = repo.svfs.lfsremoteblobstore.getlfsmetrics()
            ui.log("command_metrics", **lfsmetrics)
        except Exception:
            pass

        maxrss = util.getmaxrss()

        # Log maxrss from within the hg process. The wrapper logs its own
        # value (which is incorrect if chg is used) so the column is
        # prefixed.
        ui.log("command_info", hg_maxrss=maxrss)
    except Exception as e:
        ui.log("command_info", sampling_failure=str(e))


def reposetup(ui, repo):
    # Don't setup telemetry for sshpeer's
    if not isinstance(repo, localrepo.localrepository):
        return

    repo.ui.atexit(telemetry, weakref.ref(repo))

    # Log other information that we don't want to log in the wrapper, if it's
    # cheap to do so.

    # Log the current directory bucketed to top-level directories, if enabled.
    # This provides a very rough approximation of what area the users works in.
    # developer config: sampling.logtopdir
    if repo.ui.config("sampling", "logtopdir"):
        topdir = gettopdir(repo)
        if topdir:
            ui.log("command_info", topdir=topdir)

    # Allow environment variables to be directly mapped to metrics columns.
    env = encoding.environ
    tolog = {}
    for conf in ui.configlist("sampling", "env_vars"):
        if conf in env:
            # The default name is a lowercased version of the environment
            # variable name; in the future, an override config could be used to
            # customize it.
            tolog["env_" + conf.lower()] = env[conf]
    ui.log("env_vars", **tolog)
