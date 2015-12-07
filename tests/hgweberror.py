# A dummy extension that installs an hgweb command that throws an Exception.

from __future__ import absolute_import

from mercurial.hgweb import (
    webcommands,
)

def raiseerror(web, req, tmpl):
    '''Dummy web command that raises an uncaught Exception.'''

    # Simulate an error after partial response.
    if 'partialresponse' in req.form:
        req.respond(200, 'text/plain')
        req.write('partial content\n')

    raise AttributeError('I am an uncaught error!')

def extsetup(ui):
    setattr(webcommands, 'raiseerror', raiseerror)
    webcommands.__all__.append('raiseerror')
