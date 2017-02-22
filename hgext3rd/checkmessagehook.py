import string

from mercurial.i18n import _

def checkcommitmessage(ui, repo, **kwargs):
    """
    Checks a single commit message for adherence to commit message rules.

    To use add the following to your project .hg/hgrc for each
    project you want to check, or to your user hgrc to apply to all projects.

    [hooks]
    pretxncommit = python:path/to/script/checkmessagehook.py:checkcommitmessage
    """
    hg_commit_message = repo['tip'].description()
    try:
        hg_commit_message.decode('utf8')
    except UnicodeDecodeError:
        ui.warn(_('commit message is not utf-8\n'))
        return True

    printable = set(string.printable)
    for c in hg_commit_message:
        if ord(c) < 128 and c not in printable:
            ui.warn(_('non-printable characters in commit message\n'))
            return True

    # False means success
    return False
