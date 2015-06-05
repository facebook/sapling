# sslutil.py - SSL handling for mercurial
#
# Copyright 2005, 2006, 2007, 2008 Matt Mackall <mpm@selenic.com>
# Copyright 2006, 2007 Alexis S. L. Carvalho <alexis@cecm.usp.br>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
import os, sys

from mercurial import util
from mercurial.i18n import _

_canloaddefaultcerts = False
try:
    # avoid using deprecated/broken FakeSocket in python 2.6
    import ssl
    CERT_REQUIRED = ssl.CERT_REQUIRED
    try:
        ssl_context = ssl.SSLContext
        _canloaddefaultcerts = util.safehasattr(ssl_context,
                                                'load_default_certs')

        def wrapsocket(sock, keyfile, certfile, ui,
                       cert_reqs=ssl.CERT_NONE,
                       ca_certs=None, serverhostname=None):
            # Allow any version of SSL starting with TLSv1 and
            # up. Note that specifying TLSv1 here prohibits use of
            # newer standards (like TLSv1_2), so this is the right way
            # to do this. Note that in the future it'd be better to
            # support using ssl.create_default_context(), which sets
            # up a bunch of things in smart ways (strong ciphers,
            # protocol versions, etc) and is upgraded by Python
            # maintainers for us, but that breaks too many things to
            # do it in a hurry.
            sslcontext = ssl.SSLContext(ssl.PROTOCOL_SSLv23)
            sslcontext.options &= ssl.OP_NO_SSLv2 & ssl.OP_NO_SSLv3
            if certfile is not None:
                def password():
                    f = keyfile or certfile
                    return ui.getpass(_('passphrase for %s: ') % f, '')
                sslcontext.load_cert_chain(certfile, keyfile, password)
            sslcontext.verify_mode = cert_reqs
            if ca_certs is not None:
                sslcontext.load_verify_locations(cafile=ca_certs)
            elif _canloaddefaultcerts:
                sslcontext.load_default_certs()

            sslsocket = sslcontext.wrap_socket(sock,
                                               server_hostname=serverhostname)
            # check if wrap_socket failed silently because socket had been
            # closed
            # - see http://bugs.python.org/issue13721
            if not sslsocket.cipher():
                raise util.Abort(_('ssl connection failed'))
            return sslsocket
    except AttributeError:
        def wrapsocket(sock, keyfile, certfile, ui,
                       cert_reqs=ssl.CERT_NONE,
                       ca_certs=None, serverhostname=None):
            sslsocket = ssl.wrap_socket(sock, keyfile, certfile,
                                        cert_reqs=cert_reqs, ca_certs=ca_certs,
                                        ssl_version=ssl.PROTOCOL_TLSv1)
            # check if wrap_socket failed silently because socket had been
            # closed
            # - see http://bugs.python.org/issue13721
            if not sslsocket.cipher():
                raise util.Abort(_('ssl connection failed'))
            return sslsocket
except ImportError:
    CERT_REQUIRED = 2

    import socket, httplib

    def wrapsocket(sock, keyfile, certfile, ui,
                   cert_reqs=CERT_REQUIRED,
                   ca_certs=None, serverhostname=None):
        if not util.safehasattr(socket, 'ssl'):
            raise util.Abort(_('Python SSL support not found'))
        if ca_certs:
            raise util.Abort(_(
                'certificate checking requires Python 2.6'))

        ssl = socket.ssl(sock, keyfile, certfile)
        return httplib.FakeSocket(sock, ssl)

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
#
# We COMPLETELY ignore CERT_REQUIRED on Python <= 2.5, as it's totally
# busted on those versions.

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
            raise util.Abort(_('could not find web.cacerts: %s') % cacerts)
    else:
        cacerts = _defaultcacerts()
        if cacerts and cacerts != '!':
            ui.debug('using %s to enable OS X system CA\n' % cacerts)
        ui.setconfig('web', 'cacerts', cacerts, 'defaultcacerts')
    if cacerts != '!':
        kws.update({'ca_certs': cacerts,
                    'cert_reqs': CERT_REQUIRED,
                    })
    return kws

class validator(object):
    def __init__(self, ui, host):
        self.ui = ui
        self.host = host

    def __call__(self, sock, strict=False):
        host = self.host
        cacerts = self.ui.config('web', 'cacerts')
        hostfingerprint = self.ui.config('hostfingerprints', host)
        if not getattr(sock, 'getpeercert', False): # python 2.5 ?
            if hostfingerprint:
                raise util.Abort(_("host fingerprint for %s can't be "
                                   "verified (Python too old)") % host)
            if strict:
                raise util.Abort(_("certificate for %s can't be verified "
                                   "(Python too old)") % host)
            if self.ui.configbool('ui', 'reportoldssl', True):
                self.ui.warn(_("warning: certificate for %s can't be verified "
                               "(Python too old)\n") % host)
            return

        if not sock.cipher(): # work around http://bugs.python.org/issue13721
            raise util.Abort(_('%s ssl connection error') % host)
        try:
            peercert = sock.getpeercert(True)
            peercert2 = sock.getpeercert()
        except AttributeError:
            raise util.Abort(_('%s ssl connection error') % host)

        if not peercert:
            raise util.Abort(_('%s certificate error: '
                               'no certificate received') % host)
        peerfingerprint = util.sha1(peercert).hexdigest()
        nicefingerprint = ":".join([peerfingerprint[x:x + 2]
            for x in xrange(0, len(peerfingerprint), 2)])
        if hostfingerprint:
            if peerfingerprint.lower() != \
                    hostfingerprint.replace(':', '').lower():
                raise util.Abort(_('certificate for %s has unexpected '
                                   'fingerprint %s') % (host, nicefingerprint),
                                 hint=_('check hostfingerprint configuration'))
            self.ui.debug('%s certificate matched fingerprint %s\n' %
                          (host, nicefingerprint))
        elif cacerts != '!':
            msg = _verifycert(peercert2, host)
            if msg:
                raise util.Abort(_('%s certificate error: %s') % (host, msg),
                                 hint=_('configure hostfingerprint %s or use '
                                        '--insecure to connect insecurely') %
                                      nicefingerprint)
            self.ui.debug('%s certificate successfully verified\n' % host)
        elif strict:
            raise util.Abort(_('%s certificate with fingerprint %s not '
                               'verified') % (host, nicefingerprint),
                             hint=_('check hostfingerprints or web.cacerts '
                                     'config setting'))
        else:
            self.ui.warn(_('warning: %s certificate with fingerprint %s not '
                           'verified (check hostfingerprints or web.cacerts '
                           'config setting)\n') %
                         (host, nicefingerprint))
