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

def _hostsettings(ui, hostname):
    """Obtain security settings for a hostname.

    Returns a dict of settings relevant to that hostname.
    """
    s = {
        # List of 2-tuple of (hash algorithm, hash).
        'certfingerprints': [],
        # Path to file containing concatenated CA certs. Used by
        # SSLContext.load_verify_locations().
        'cafile': None,
        # ssl.CERT_* constant used by SSLContext.verify_mode.
        'verifymode': None,
    }

    # Fingerprints from [hostfingerprints] are always SHA-1.
    for fingerprint in ui.configlist('hostfingerprints', hostname, []):
        fingerprint = fingerprint.replace(':', '').lower()
        s['certfingerprints'].append(('sha1', fingerprint))

    # If a host cert fingerprint is defined, it is the only thing that
    # matters. No need to validate CA certs.
    if s['certfingerprints']:
        s['verifymode'] = ssl.CERT_NONE

    # If --insecure is used, don't take CAs into consideration.
    elif ui.insecureconnections:
        s['verifymode'] = ssl.CERT_NONE

    # Try to hook up CA certificate validation unless something above
    # makes it not necessary.
    if s['verifymode'] is None:
        # Find global certificates file in config.
        cafile = ui.config('web', 'cacerts')

        if cafile:
            cafile = util.expandpath(cafile)
            if not os.path.exists(cafile):
                raise error.Abort(_('could not find web.cacerts: %s') % cafile)
        else:
            # No global CA certs. See if we can load defaults.
            cafile = _defaultcacerts()
            if cafile:
                ui.debug('using %s to enable OS X system CA\n' % cafile)

        s['cafile'] = cafile

        # Require certificate validation if CA certs are being loaded and
        # verification hasn't been disabled above.
        if cafile or _canloaddefaultcerts:
            s['verifymode'] = ssl.CERT_REQUIRED
        else:
            # At this point we don't have a fingerprint, aren't being
            # explicitly insecure, and can't load CA certs. Connecting
            # at this point is insecure. But we do it for BC reasons.
            # TODO abort here to make secure by default.
            s['verifymode'] = ssl.CERT_NONE

    assert s['verifymode'] is not None

    return s

def wrapsocket(sock, keyfile, certfile, ui, serverhostname=None):
    """Add SSL/TLS to a socket.

    This is a glorified wrapper for ``ssl.wrap_socket()``. It makes sane
    choices based on what security options are available.

    In addition to the arguments supported by ``ssl.wrap_socket``, we allow
    the following additional arguments:

    * serverhostname - The expected hostname of the remote server. If the
      server (and client) support SNI, this tells the server which certificate
      to use.
    """
    if not serverhostname:
        raise error.Abort('serverhostname argument is required')

    settings = _hostsettings(ui, serverhostname)

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
    sslcontext.verify_mode = settings['verifymode']

    if certfile is not None:
        def password():
            f = keyfile or certfile
            return ui.getpass(_('passphrase for %s: ') % f, '')
        sslcontext.load_cert_chain(certfile, keyfile, password)

    if settings['cafile'] is not None:
        sslcontext.load_verify_locations(cafile=settings['cafile'])
        caloaded = True
    else:
        # This is a no-op on old Python.
        sslcontext.load_default_certs()
        caloaded = _canloaddefaultcerts

    sslsocket = sslcontext.wrap_socket(sock, server_hostname=serverhostname)
    # check if wrap_socket failed silently because socket had been
    # closed
    # - see http://bugs.python.org/issue13721
    if not sslsocket.cipher():
        raise error.Abort(_('ssl connection failed'))

    sslsocket._hgstate = {
        'caloaded': caloaded,
        'hostname': serverhostname,
        'settings': settings,
        'ui': ui,
    }

    return sslsocket

def _verifycert(cert, hostname):
    '''Verify that cert (in socket.getpeercert() format) matches hostname.
    CRLs is not handled.

    Returns error message if any problems are found and None on success.
    '''
    if not cert:
        return _('no certificate received')
    dnsname = hostname.lower()
    def matchdnsname(certname):
        return (certname == dnsname or
                '.' in dnsname and certname == '*.' + dnsname.split('.', 1)[1])

    san = cert.get('subjectAltName', [])
    if san:
        certnames = [value.lower() for key, value in san if key == 'DNS']
        for name in certnames:
            if matchdnsname(name):
                return None
        if certnames:
            return _('certificate is for %s') % ', '.join(certnames)

    # subject is only checked when subjectAltName is empty
    for s in cert.get('subject', []):
        key, value = s[0]
        if key == 'commonName':
            try:
                # 'subject' entries are unicode
                certname = value.lower().encode('ascii')
            except UnicodeEncodeError:
                return _('IDN in certificate not supported')
            if matchdnsname(certname):
                return None
            return _('certificate is for %s') % certname
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
    """return path to default CA certificates or None."""
    if _plainapplepython():
        dummycert = os.path.join(os.path.dirname(__file__), 'dummycert.pem')
        if os.path.exists(dummycert):
            return dummycert

    return None

def validatesocket(sock, strict=False):
    """Validate a socket meets security requiremnets.

    The passed socket must have been created with ``wrapsocket()``.
    """
    host = sock._hgstate['hostname']
    ui = sock._hgstate['ui']
    settings = sock._hgstate['settings']

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
    peerfingerprint = util.sha1(peercert).hexdigest()
    nicefingerprint = ":".join([peerfingerprint[x:x + 2]
        for x in xrange(0, len(peerfingerprint), 2)])
    if settings['certfingerprints']:
        fingerprintmatch = False
        for hash, fingerprint in settings['certfingerprints']:
            if peerfingerprint.lower() == fingerprint:
                fingerprintmatch = True
                break
        if not fingerprintmatch:
            raise error.Abort(_('certificate for %s has unexpected '
                               'fingerprint %s') % (host, nicefingerprint),
                             hint=_('check hostfingerprint configuration'))
        ui.debug('%s certificate matched fingerprint %s\n' %
                 (host, nicefingerprint))
        return

    # If insecure connections were explicitly requested via --insecure,
    # print a warning and do no verification.
    #
    # It may seem odd that this is checked *after* host fingerprint pinning.
    # This is for backwards compatibility (for now). The message is also
    # the same as below for BC.
    if ui.insecureconnections:
        ui.warn(_('warning: %s certificate with fingerprint %s not '
                  'verified (check hostfingerprints or web.cacerts '
                  'config setting)\n') %
                (host, nicefingerprint))
        return

    if not sock._hgstate['caloaded']:
        if strict:
            raise error.Abort(_('%s certificate with fingerprint %s not '
                                'verified') % (host, nicefingerprint),
                              hint=_('check hostfingerprints or '
                                     'web.cacerts config setting'))
        else:
            ui.warn(_('warning: %s certificate with fingerprint %s '
                      'not verified (check hostfingerprints or '
                      'web.cacerts config setting)\n') %
                    (host, nicefingerprint))

        return

    msg = _verifycert(peercert2, host)
    if msg:
        raise error.Abort(_('%s certificate error: %s') % (host, msg),
                         hint=_('configure hostfingerprint %s or use '
                                '--insecure to connect insecurely') %
                              nicefingerprint)
