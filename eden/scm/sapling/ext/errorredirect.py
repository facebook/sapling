# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""redirect error message

Redirect error message, the stack trace, of an uncaught exception to
a custom shell script. This is useful for further handling the error,
for example posting it to a support group and logging it somewhere.

The config option errorredirect.script is the shell script to execute.
If it's empty, the extension will do nothing and fallback to the old
behavior.

Two environment variables are set: TRACE is the stack trace, which
is the same as piped content. WARNING is the warning message, which
usually contains contact message and software versions, etc.

Examples::

  [errorredirect]
  script = tee hgerr.log && echo 'Error written to hgerr.log'

  [errorredirect]
  script = echo "$WARNING$TRACE" >&2

  [errorredirect]
  script = (echo "$WARNING"; cat) | cat >&2
"""

import signal
import subprocess
import sys
import traceback

from sapling import alerts, dispatch, encoding, extensions, util


def _printtrace(ui, warning) -> bool:
    # Like dispatch.handlecommandexception, but avoids an unnecessary ui.log
    ui.warn(warning)
    return False  # return value for "handlecommandexception", re-raises


def _handlecommandexception(orig, ui):
    warning = dispatch._exceptionwarning(ui)
    if ui.configbool("errorredirect", "fancy-traceback"):
        trace = util.smartformatexc()
    else:
        trace = traceback.format_exc()

    # let blackbox log it (if it is configured to do so)
    ui.log("command_exception", "%s\n%s\n", warning, trace)
    exctype = sys.exc_info()[0]
    exctypename = "None" if exctype is None else exctype.__name__
    ui.log_exception(
        "exception has occurred: %s",
        warning,
        exception_type=exctypename,
        exception_msg=str(sys.exc_info()[1]),
        source="command_exception",
        traceback=trace,
    )

    script = ui.config("errorredirect", "script")
    if not script:
        return orig(ui)

    alerts.print_matching_alerts_for_exception(ui, trace)

    # run the external script
    env = encoding.environ.copy()
    env["WARNING"] = warning
    env["TRACE"] = trace

    # decide whether to use shell smartly, see 9335dc6b2a9c in hg
    shell = any(c in script for c in "|&;<>()$`\\\"' \t\n*?[#~=%")

    try:
        p = subprocess.Popen(script, shell=shell, stdin=subprocess.PIPE, env=env)
        p.communicate(trace.encode())
    except Exception:
        # The binary cannot be executed, or some other issues. For example,
        # "script" is not in PATH, and shell is False; or the peer closes the
        # pipe early. Fallback to the plain error reporting.
        return _printtrace(ui, warning)
    else:
        ret = p.returncode

        # Python returns negative exit code for signal-terminated process. The
        # shell converts singal-terminated process to a positive exit code by
        # +128. Ctrl+C generates SIGTERM. Re-report the error unless the
        # process exits cleanly or is terminated by SIGTERM (Ctrl+C).
        ctrlc = (ret == signal.SIGTERM + 128) or (ret == -signal.SIGTERM)
        if ret != 0 and not ctrlc:
            return _printtrace(ui, warning)

    return True  # do not re-raise


def uisetup(ui) -> None:
    extensions.wrapfunction(dispatch, "handlecommandexception", _handlecommandexception)
