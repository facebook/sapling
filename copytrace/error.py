from mercurial.i18n import _


def logfailure(repo, e, funcname):
    """
    logging the error in the blackbox and ask user to report
    """
    ui = repo.ui
    log = funcname + '\n' + ''.join([str(arg) + '\n' for arg in e.args])
    ui.log('copytrace', log)
    warnmsg = ui.config('copytrace', 'exceptionmsg',
            _("** unknown exception encountered with copytracing **\n"))
    repo.ui.warn(warnmsg)
