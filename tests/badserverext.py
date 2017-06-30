# badserverext.py - Extension making servers behave badly
#
# Copyright 2017 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# no-check-code

"""Extension to make servers behave badly.

This extension is useful for testing Mercurial behavior when various network
events occur.

Various config options in the [badserver] section influence behavior:

closebeforeaccept
   If true, close() the server socket when a new connection arrives before
   accept() is called. The server will then exit.

closeafteraccept
   If true, the server will close() the client socket immediately after
   accept().

closeafterrecvbytes
   If defined, close the client socket after receiving this many bytes.

closeaftersendbytes
   If defined, close the client socket after sending this many bytes.
"""

from __future__ import absolute_import

import socket

from mercurial import(
    registrar,
)

from mercurial.hgweb import (
    server,
)

configtable = {}
configitem = registrar.configitem(configtable)

configitem('badserver', 'closeafteraccept',
    default=False,
)
configitem('badserver', 'closeafterrecvbytes',
    default=0,
)
configitem('badserver', 'closeaftersendbytes',
    default=0,
)
configitem('badserver', 'closebeforeaccept',
    default=False,
)

# We can't adjust __class__ on a socket instance. So we define a proxy type.
class socketproxy(object):
    __slots__ = (
        '_orig',
        '_logfp',
        '_closeafterrecvbytes',
        '_closeaftersendbytes',
    )

    def __init__(self, obj, logfp, closeafterrecvbytes=0,
                 closeaftersendbytes=0):
        object.__setattr__(self, '_orig', obj)
        object.__setattr__(self, '_logfp', logfp)
        object.__setattr__(self, '_closeafterrecvbytes', closeafterrecvbytes)
        object.__setattr__(self, '_closeaftersendbytes', closeaftersendbytes)

    def __getattribute__(self, name):
        if name in ('makefile',):
            return object.__getattribute__(self, name)

        return getattr(object.__getattribute__(self, '_orig'), name)

    def __delattr__(self, name):
        delattr(object.__getattribute__(self, '_orig'), name)

    def __setattr__(self, name, value):
        setattr(object.__getattribute__(self, '_orig'), name, value)

    def makefile(self, mode, bufsize):
        f = object.__getattribute__(self, '_orig').makefile(mode, bufsize)

        logfp = object.__getattribute__(self, '_logfp')
        closeafterrecvbytes = object.__getattribute__(self,
                                                      '_closeafterrecvbytes')
        closeaftersendbytes = object.__getattribute__(self,
                                                      '_closeaftersendbytes')

        return fileobjectproxy(f, logfp,
                               closeafterrecvbytes=closeafterrecvbytes,
                               closeaftersendbytes=closeaftersendbytes)

# We can't adjust __class__ on socket._fileobject, so define a proxy.
class fileobjectproxy(object):
    __slots__ = (
        '_orig',
        '_logfp',
        '_closeafterrecvbytes',
        '_closeaftersendbytes',
    )

    def __init__(self, obj, logfp, closeafterrecvbytes=0,
                 closeaftersendbytes=0):
        object.__setattr__(self, '_orig', obj)
        object.__setattr__(self, '_logfp', logfp)
        object.__setattr__(self, '_closeafterrecvbytes', closeafterrecvbytes)
        object.__setattr__(self, '_closeaftersendbytes', closeaftersendbytes)

    def __getattribute__(self, name):
        if name in ('read', 'readline', 'write', '_writelog'):
            return object.__getattribute__(self, name)

        return getattr(object.__getattribute__(self, '_orig'), name)

    def __delattr__(self, name):
        delattr(object.__getattribute__(self, '_orig'), name)

    def __setattr__(self, name, value):
        setattr(object.__getattribute__(self, '_orig'), name, value)

    def _writelog(self, msg):
        msg = msg.replace('\r', '\\r').replace('\n', '\\n')

        object.__getattribute__(self, '_logfp').write(msg)
        object.__getattribute__(self, '_logfp').write('\n')
        object.__getattribute__(self, '_logfp').flush()

    def read(self, size=-1):
        remaining = object.__getattribute__(self, '_closeafterrecvbytes')

        # No read limit. Call original function.
        if not remaining:
            result = object.__getattribute__(self, '_orig').read(size)
            self._writelog('read(%d) -> (%d) (%s) %s' % (size,
                                                           len(result),
                                                           result))
            return result

        origsize = size

        if size < 0:
            size = remaining
        else:
            size = min(remaining, size)

        result = object.__getattribute__(self, '_orig').read(size)
        remaining -= len(result)

        self._writelog('read(%d from %d) -> (%d) %s' % (
            size, origsize, len(result), result))

        object.__setattr__(self, '_closeafterrecvbytes', remaining)

        if remaining <= 0:
            self._writelog('read limit reached, closing socket')
            self._sock.close()
            # This is the easiest way to abort the current request.
            raise Exception('connection closed after receiving N bytes')

        return result

    def readline(self, size=-1):
        remaining = object.__getattribute__(self, '_closeafterrecvbytes')

        # No read limit. Call original function.
        if not remaining:
            result = object.__getattribute__(self, '_orig').readline(size)
            self._writelog('readline(%d) -> (%d) %s' % (
                size, len(result), result))
            return result

        origsize = size

        if size < 0:
            size = remaining
        else:
            size = min(remaining, size)

        result = object.__getattribute__(self, '_orig').readline(size)
        remaining -= len(result)

        self._writelog('readline(%d from %d) -> (%d) %s' % (
            size, origsize, len(result), result))

        object.__setattr__(self, '_closeafterrecvbytes', remaining)

        if remaining <= 0:
            self._writelog('read limit reached; closing socket')
            self._sock.close()
            # This is the easiest way to abort the current request.
            raise Exception('connection closed after receiving N bytes')

        return result

    def write(self, data):
        remaining = object.__getattribute__(self, '_closeaftersendbytes')

        # No byte limit on this operation. Call original function.
        if not remaining:
            self._writelog('write(%d) -> %s' % (len(data), data))
            result = object.__getattribute__(self, '_orig').write(data)
            return result

        if len(data) > remaining:
            newdata = data[0:remaining]
        else:
            newdata = data

        remaining -= len(newdata)

        self._writelog('write(%d from %d) -> (%d) %s' % (
            len(newdata), len(data), remaining, newdata))

        result = object.__getattribute__(self, '_orig').write(newdata)

        object.__setattr__(self, '_closeaftersendbytes', remaining)

        if remaining <= 0:
            self._writelog('write limit reached; closing socket')
            self._sock.close()
            raise Exception('connection closed after sending N bytes')

        return result

def extsetup(ui):
    # Change the base HTTP server class so various events can be performed.
    # See SocketServer.BaseServer for how the specially named methods work.
    class badserver(server.MercurialHTTPServer):
        def __init__(self, ui, *args, **kwargs):
            self._ui = ui
            super(badserver, self).__init__(ui, *args, **kwargs)

            # Need to inherit object so super() works.
            class badrequesthandler(self.RequestHandlerClass, object):
                def send_header(self, name, value):
                    # Make headers deterministic to facilitate testing.
                    if name.lower() == 'date':
                        value = 'Fri, 14 Apr 2017 00:00:00 GMT'
                    elif name.lower() == 'server':
                        value = 'badhttpserver'

                    return super(badrequesthandler, self).send_header(name,
                                                                      value)

            self.RequestHandlerClass = badrequesthandler

        # Called to accept() a pending socket.
        def get_request(self):
            if self._ui.configbool('badserver', 'closebeforeaccept'):
                self.socket.close()

                # Tells the server to stop processing more requests.
                self.__shutdown_request = True

                # Simulate failure to stop processing this request.
                raise socket.error('close before accept')

            if self._ui.configbool('badserver', 'closeafteraccept'):
                request, client_address = super(badserver, self).get_request()
                request.close()
                raise socket.error('close after accept')

            return super(badserver, self).get_request()

        # Does heavy lifting of processing a request. Invokes
        # self.finish_request() which calls self.RequestHandlerClass() which
        # is a hgweb.server._httprequesthandler.
        def process_request(self, socket, address):
            # Wrap socket in a proxy if we need to count bytes.
            closeafterrecvbytes = self._ui.configint('badserver',
                                                     'closeafterrecvbytes')
            closeaftersendbytes = self._ui.configint('badserver',
                                                     'closeaftersendbytes')

            if closeafterrecvbytes or closeaftersendbytes:
                socket = socketproxy(socket, self.errorlog,
                                     closeafterrecvbytes=closeafterrecvbytes,
                                     closeaftersendbytes=closeaftersendbytes)

            return super(badserver, self).process_request(socket, address)

    server.MercurialHTTPServer = badserver
