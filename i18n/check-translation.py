#!/usr/bin/env python
#
# check-translation.py - check Mercurial specific translation problems

import polib
import re

checkers = []

def checker(level, msgidpat):
    def decorator(func):
        if msgidpat:
            match = re.compile(msgidpat).search
        else:
            match = lambda msgid: True
        checkers.append((func, level))
        func.match = match
        return func
    return decorator

def match(checker, pe):
    """Examine whether POEntry "pe" is target of specified checker or not
    """
    if not checker.match(pe.msgid):
        return
    # examine suppression by translator comment
    nochecker = 'no-%s-check' % checker.__name__
    for tc in pe.tcomment.split():
        if nochecker == tc:
            return
    return True

####################

def fatalchecker(msgidpat=None):
    return checker('fatal', msgidpat)

@fatalchecker(r'\$\$')
def promptchoice(pe):
    """Check translation of the string given to "ui.promptchoice()"

    >>> pe = polib.POEntry(
    ...     msgid ='prompt$$missing &sep$$missing &amp$$followed by &none',
    ...     msgstr='prompt  missing &sep$$missing  amp$$followed by none&')
    >>> match(promptchoice, pe)
    True
    >>> for e in promptchoice(pe): print e
    number of choices differs between msgid and msgstr
    msgstr has invalid choice missing '&'
    msgstr has invalid '&' followed by none
    """
    idchoices = [c.rstrip(' ') for c in pe.msgid.split('$$')[1:]]
    strchoices = [c.rstrip(' ') for c in pe.msgstr.split('$$')[1:]]

    if len(idchoices) != len(strchoices):
        yield "number of choices differs between msgid and msgstr"

    indices = [(c, c.find('&')) for c in strchoices]
    if [c for c, i in indices if i == -1]:
        yield "msgstr has invalid choice missing '&'"
    if [c for c, i in indices if len(c) == i + 1]:
        yield "msgstr has invalid '&' followed by none"

####################

def warningchecker(msgidpat=None):
    return checker('warning', msgidpat)

####################

def check(pofile, fatal=True, warning=False):
    targetlevel = { 'fatal': fatal, 'warning': warning }
    targetcheckers = [(checker, level)
                      for checker, level in checkers
                      if targetlevel[level]]
    if not targetcheckers:
        return []

    detected = []
    for pe in pofile.translated_entries():
        errors = []
        for checker, level in targetcheckers:
            if match(checker, pe):
                errors.extend((level, checker.__name__, error)
                              for error in checker(pe))
        if errors:
            detected.append((pe, errors))
    return detected

########################################

if __name__ == "__main__":
    import sys
    import optparse

    optparser = optparse.OptionParser("""%prog [options] pofile ...

This checks Mercurial specific translation problems in specified
'*.po' files.

Each detected problems are shown in the format below::

    filename:linenum:type(checker): problem detail .....

"type" is "fatal" or "warning". "checker" is the name of the function
detecting corresponded error.

Checking by checker "foo" on the specific msgstr can be suppressed by
the "translator comment" like below. Multiple "no-xxxx-check" should
be separated by whitespaces::

    # no-foo-check
    msgid = "....."
    msgstr = "....."
""")
    optparser.add_option("", "--warning",
                         help="show also warning level problems",
                         action="store_true")
    optparser.add_option("", "--doctest",
                         help="run doctest of this tool, instead of check",
                         action="store_true")
    (options, args) = optparser.parse_args()

    if options.doctest:
        import os
        if 'TERM' in os.environ:
            del os.environ['TERM']
        import doctest
        failures, tests = doctest.testmod()
        sys.exit(failures and 1 or 0)

    # replace polib._POFileParser to show linenum of problematic msgstr
    class ExtPOFileParser(polib._POFileParser):
        def process(self, symbol, linenum):
            super(ExtPOFileParser, self).process(symbol, linenum)
            if symbol == 'MS': # msgstr
                self.current_entry.linenum = linenum
    polib._POFileParser = ExtPOFileParser

    detected = []
    warning = options.warning
    for f in args:
        detected.extend((f, pe, errors)
                        for pe, errors in check(polib.pofile(f),
                                                warning=warning))
    if detected:
        for f, pe, errors in detected:
            for level, checker, error in errors:
                sys.stderr.write('%s:%d:%s(%s): %s\n'
                                 % (f, pe.linenum, level, checker, error))
        sys.exit(1)
