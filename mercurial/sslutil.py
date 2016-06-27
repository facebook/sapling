# sslutil.py - SSL handling for mercurial
#
# Copyright 2005, 2006, 2007, 2008 Matt Mackall <mpm@selenic.com>
# Copyright 2006, 2007 Alexis S. L. Carvalho <alexis@cecm.usp.br>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import re
import ssl
import sys

from .i18n import _
from . import (
    error,
    util,
)

# Python 2.7.9+ overhauled the built-in SSL/TLS features of Python. It added
# support for TLS 1.1, TLS 1.2, SNI, system CA stores, etc. These features are
# all exposed via the "ssl" module.
#
# Depending on the version of Python being used, SSL/TLS support is either
# modern/secure or legacy/insecure. Many operations in this module have
# separate code paths depending on support in Python.

hassni = getattr(ssl, 'HAS_SNI', False)

try:
    OP_NO_SSLv2 = ssl.OP_NO_SSLv2
    OP_NO_SSLv3 = ssl.OP_NO_SSLv3
except AttributeError:
    OP_NO_SSLv2 = 0x1000000
    OP_NO_SSLv3 = 0x2000000

try:
    # ssl.SSLContext was added in 2.7.9 and presence indicates modern
    # SSL/TLS features are available.
    SSLContext = ssl.SSLContext
    modernssl = True
    _canloaddefaultcerts = util.safehasattr(SSLContext, 'load_default_certs')
except AttributeError:
    modernssl = False
    _canloaddefaultcerts = False

    # We implement SSLContext using the interface from the standard library.
    class SSLContext(object):
        # ssl.wrap_socket gained the "ciphers" named argument in 2.7.
        _supportsciphers = sys.version_info >= (2, 7)

        def __init__(self, protocol):
            # From the public interface of SSLContext
            self.protocol = protocol
            self.check_hostname = False
            self.options = 0
            self.verify_mode = ssl.CERT_NONE

            # Used by our implementation.
            self._certfile = None
            self._keyfile = None
            self._certpassword = None
            self._cacerts = None
            self._ciphers = None

        def load_cert_chain(self, certfile, keyfile=None, password=None):
            self._certfile = certfile
            self._keyfile = keyfile
            self._certpassword = password

        def load_default_certs(self, purpose=None):
            pass

        def load_verify_locations(self, cafile=None, capath=None, cadata=None):
            if capath:
                raise error.Abort('capath not supported')
            if cadata:
                raise error.Abort('cadata not supported')

            self._cacerts = cafile

        def set_ciphers(self, ciphers):
            if not self._supportsciphers:
                raise error.Abort('setting ciphers not supported')

            self._ciphers = ciphers

        def wrap_socket(self, socket, server_hostname=None, server_side=False):
            # server_hostname is unique to SSLContext.wrap_socket and is used
            # for SNI in that context. So there's nothing for us to do with it
            # in this legacy code since we don't support SNI.

            args = {
                'keyfile': self._keyfile,
                'certfile': self._certfile,
                'server_side': server_side,
                'cert_reqs': self.verify_mode,
                'ssl_version': self.protocol,
                'ca_certs': self._cacerts,
            }

            if self._supportsciphers:
                args['ciphers'] = self._ciphers

            return ssl.wrap_socket(socket, **args)

def wrapsocket(sock, keyfile, certfile, ui, cert_reqs=ssl.CERT_NONE,
               ca_certs=None, serverhostname=None):
    """Add SSL/TLS to a socket.

    This is a glorified wrapper for ``ssl.wrap_socket()``. It makes sane
    choices based on what security options are available.

    In addition to the arguments supported by ``ssl.wrap_socket``, we allow
    the following additional arguments:

    * serverhostname - The expected hostname of the remote server. If the
      server (and client) support SNI, this tells the server which certificate
      to use.
    """
    # Despite its name, PROTOCOL_SSLv23 selects the highest protocol
    # that both ends support, including TLS protocols. On legacy stacks,
    # the highest it likely goes in TLS 1.0. On modern stacks, it can
    # support TLS 1.2.
    #
    # The PROTOCOL_TLSv* constants select a specific TLS version
    # only (as opposed to multiple versions). So the method for
    # supporting multiple TLS versions is to use PROTOCOL_SSLv23 and
    # disable protocols via SSLContext.options and OP_NO_* constants.
    # However, SSLContext.options doesn't work unless we have the
    # full/real SSLContext available to us.
    #
    # SSLv2 and SSLv3 are broken. We ban them outright.
    if modernssl:
        protocol = ssl.PROTOCOL_SSLv23
    else:
        protocol = ssl.PROTOCOL_TLSv1

    # TODO use ssl.create_default_context() on modernssl.
    sslcontext = SSLContext(protocol)

    # This is a no-op on old Python.
    sslcontext.options |= OP_NO_SSLv2 | OP_NO_SSLv3

    # This still works on our fake SSLContext.
    sslcontext.verify_mode = cert_reqs

    if certfile is not None:
        def password():
            f = keyfile or certfile
            return ui.getpass(_('passphrase for %s: ') % f, '')
        sslcontext.load_cert_chain(certfile, keyfile, password)

    if ca_certs is not None:
        sslcontext.load_verify_locations(cafile=ca_certs)
    else:
        # This is a no-op on old Python.
        sslcontext.load_default_certs()

    sslsocket = sslcontext.wrap_socket(sock, server_hostname=serverhostname)
    # check if wrap_socket failed silently because socket had been
    # closed
    # - see http://bugs.python.org/issue13721
    if not sslsocket.cipher():
        raise error.Abort(_('ssl connection failed'))
    return sslsocket

class wildcarderror(Exception):
    """Represents an error parsing wildcards in DNS name."""

def _dnsnamematch(dn, hostname, maxwildcards=1):
    """Match DNS names according RFC 6125 section 6.4.3.

    This code is effectively copied from CPython's ssl._dnsname_match.

    Returns a bool indicating whether the expected hostname matches
    the value in ``dn``.
    """
    pats = []
    if not dn:
        return False

    pieces = dn.split(r'.')
    leftmost = pieces[0]
    remainder = pieces[1:]
    wildcards = leftmost.count('*')
    if wildcards > maxwildcards:
        raise wildcarderror(
            _('too many wildcards in certificate DNS name: %s') % dn)

    # speed up common case w/o wildcards
    if not wildcards:
        return dn.lower() == hostname.lower()

    # RFC 6125, section 6.4.3, subitem 1.
    # The client SHOULD NOT attempt to match a presented identifier in which
    # the wildcard character comprises a label other than the left-most label.
    if leftmost == '*':
        # When '*' is a fragment by itself, it matches a non-empty dotless
        # fragment.
        pats.append('[^.]+')
    elif leftmost.startswith('xn--') or hostname.startswith('xn--'):
        # RFC 6125, section 6.4.3, subitem 3.
        # The client SHOULD NOT attempt to match a presented identifier
        # where the wildcard character is embedded within an A-label or
        # U-label of an internationalized domain name.
        pats.append(re.escape(leftmost))
    else:
        # Otherwise, '*' matches any dotless string, e.g. www*
        pats.append(re.escape(leftmost).replace(r'\*', '[^.]*'))

    # add the remaining fragments, ignore any wildcards
    for frag in remainder:
        pats.append(re.escape(frag))

    pat = re.compile(r'\A' + r'\.'.join(pats) + r'\Z', re.IGNORECASE)
    return pat.match(hostname) is not None

def _verifycert(cert, hostname):
    '''Verify that cert (in socket.getpeercert() format) matches hostname.
    CRLs is not handled.

    Returns error message if any problems are found and None on success.
    '''
    if not cert:
        return _('no certificate received')

    dnsnames = []
    san = cert.get('subjectAltName', [])
    for key, value in san:
        if key == 'DNS':
            try:
                if _dnsnamematch(value, hostname):
                    return
            except wildcarderror as e:
                return e.message

            dnsnames.append(value)

    if not dnsnames:
        # The subject is only checked when there is no DNS in subjectAltName.
        for sub in cert.get('subject', []):
            for key, value in sub:
                # According to RFC 2818 the most specific Common Name must
                # be used.
                if key == 'commonName':
                    # 'subject' entries are unicide.
                    try:
                        value = value.encode('ascii')
                    except UnicodeEncodeError:
                        return _('IDN in certificate not supported')

                    try:
                        if _dnsnamematch(value, hostname):
                            return
                    except wildcarderror as e:
                        return e.message

                    dnsnames.append(value)

    if len(dnsnames) > 1:
        return _('certificate is for %s') % ', '.join(dnsnames)
    elif len(dnsnames) == 1:
        return _('certificate is for %s') % dnsnames[0]
    else:
        return _('no commonName or subjectAltName found in certificate')


# CERT_REQUIRED means fetch the cert from the server all the time AND
# validate it against the CA store provided in web.cacerts.

def _plainapplepython():
    """return true if this seems to be a pure Apple Python that
    * is unfrozen and presumably has the whole mercurial module in the file
      system
    * presumably is an Apple Python that uses Apple OpenSSL which has patches
      for using system certificate store CAs in addition to the provided
      cacerts file
    """
    if sys.platform != 'darwin' or util.mainfrozen() or not sys.executable:
        return False
    exe = os.path.realpath(sys.executable).lower()
    return (exe.startswith('/usr/bin/python') or
            exe.startswith('/system/library/frameworks/python.framework/'))

def _defaultcacerts():
    """return path to CA certificates; None for system's store; ! to disable"""
    if _plainapplepython():
        dummycert = os.path.join(os.path.dirname(__file__), 'dummycert.pem')
        if os.path.exists(dummycert):
            return dummycert
    if _canloaddefaultcerts:
        return None
    return '!'

def sslkwargs(ui, host):
    kws = {'ui': ui}
    hostfingerprint = ui.config('hostfingerprints', host)
    if hostfingerprint:
        return kws
    cacerts = ui.config('web', 'cacerts')
    if cacerts == '!':
        pass
    elif cacerts:
        cacerts = util.expandpath(cacerts)
        if not os.path.exists(cacerts):
            raise error.Abort(_('could not find web.cacerts: %s') % cacerts)
    else:
        cacerts = _defaultcacerts()
        if cacerts and cacerts != '!':
            ui.debug('using %s to enable OS X system CA\n' % cacerts)
        ui.setconfig('web', 'cacerts', cacerts, 'defaultcacerts')
    if cacerts != '!':
        kws.update({'ca_certs': cacerts,
                    'cert_reqs': ssl.CERT_REQUIRED,
                    })
    return kws

class validator(object):
    def __init__(self, ui, host):
        self.ui = ui
        self.host = host

    def __call__(self, sock, strict=False):
        host = self.host

        if not sock.cipher(): # work around http://bugs.python.org/issue13721
            raise error.Abort(_('%s ssl connection error') % host)
        try:
            peercert = sock.getpeercert(True)
            peercert2 = sock.getpeercert()
        except AttributeError:
            raise error.Abort(_('%s ssl connection error') % host)

        if not peercert:
            raise error.Abort(_('%s certificate error: '
                               'no certificate received') % host)

        # If a certificate fingerprint is pinned, use it and only it to
        # validate the remote cert.
        hostfingerprints = self.ui.configlist('hostfingerprints', host)
        peerfingerprint = util.sha1(peercert).hexdigest()
        nicefingerprint = ":".join([peerfingerprint[x:x + 2]
            for x in xrange(0, len(peerfingerprint), 2)])
        if hostfingerprints:
            fingerprintmatch = False
            for hostfingerprint in hostfingerprints:
                if peerfingerprint.lower() == \
                        hostfingerprint.replace(':', '').lower():
                    fingerprintmatch = True
                    break
            if not fingerprintmatch:
                raise error.Abort(_('certificate for %s has unexpected '
                                   'fingerprint %s') % (host, nicefingerprint),
                                 hint=_('check hostfingerprint configuration'))
            self.ui.debug('%s certificate matched fingerprint %s\n' %
                          (host, nicefingerprint))
            return

        # No pinned fingerprint. Establish trust by looking at the CAs.
        cacerts = self.ui.config('web', 'cacerts')
        if cacerts != '!':
            msg = _verifycert(peercert2, host)
            if msg:
                raise error.Abort(_('%s certificate error: %s') % (host, msg),
                                 hint=_('configure hostfingerprint %s or use '
                                        '--insecure to connect insecurely') %
                                      nicefingerprint)
            self.ui.debug('%s certificate successfully verified\n' % host)
        elif strict:
            raise error.Abort(_('%s certificate with fingerprint %s not '
                               'verified') % (host, nicefingerprint),
                             hint=_('check hostfingerprints or web.cacerts '
                                     'config setting'))
        else:
            self.ui.warn(_('warning: %s certificate with fingerprint %s not '
                           'verified (check hostfingerprints or web.cacerts '
                           'config setting)\n') %
                         (host, nicefingerprint))
