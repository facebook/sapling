from mercurial import util, ui
from mercurial.i18n import gettext as _
import re

# regexp for single LF without CR preceding.
re_single_lf = re.compile('(^|[^\r])\n', re.MULTILINE)

def dumbdecode(s, cmd):
    # warn if already has CRLF in repository.
    # it might cause unexpected eol conversion.
    # see issue 302:
    #   http://www.selenic.com/mercurial/bts/issue302
    if '\r\n' in s:
        u = ui.ui()
        u.warn(_('WARNING: file in repository already has CRLF line ending \n'
                 ' which does not need eol conversion by win32text plugin.\n'
                 ' Please reconsider encode/decode setting in'
                 ' mercurial.ini or .hg/hgrc\n'
                 ' before next commit.\n'))
    # replace single LF to CRLF
    return re_single_lf.sub('\\1\r\n', s)

def dumbencode(s, cmd):
    return s.replace('\r\n', '\n')

def clevertest(s, cmd):
    if '\0' in s: return False
    return True

def cleverdecode(s, cmd):
    if clevertest(s, cmd):
        return dumbdecode(s, cmd)
    return s

def cleverencode(s, cmd):
    if clevertest(s, cmd):
        return dumbencode(s, cmd)
    return s

util.filtertable.update({
    'dumbdecode:': dumbdecode,
    'dumbencode:': dumbencode,
    'cleverdecode:': cleverdecode,
    'cleverencode:': cleverencode,
    })
