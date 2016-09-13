# sslutil.py - SSL handling for mercurial
#
# Copyright 2005, 2006, 2007, 2008 Matt Mackall <mpm@selenic.com>
# Copyright 2006, 2007 Alexis S. L. Carvalho <alexis@cecm.usp.br>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import hashlib
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

configprotocols = set([
    'tls1.0',
    'tls1.1',
    'tls1.2',
])

hassni = getattr(ssl, 'HAS_SNI', False)

# TLS 1.1 and 1.2 may not be supported if the OpenSSL Python is compiled
# against doesn't support them.
supportedprotocols = set(['tls1.0'])
if util.safehasattr(ssl, 'PROTOCOL_TLSv1_1'):
    supportedprotocols.add('tls1.1')
if util.safehasattr(ssl, 'PROTOCOL_TLSv1_2'):
    supportedprotocols.add('tls1.2')

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
                raise error.Abort(_('capath not supported'))
            if cadata:
                raise error.Abort(_('cadata not supported'))

            self._cacerts = cafile

        def set_ciphers(self, ciphers):
            if not self._supportsciphers:
                raise error.Abort(_('setting ciphers in [hostsecurity] is not '
                                    'supported by this version of Python'),
                                  hint=_('remove the config option or run '
                                         'Mercurial with a modern Python '
                                         'version (preferred)'))

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
        # Whether we should attempt to load default/available CA certs
        # if an explicit ``cafile`` is not defined.
        'allowloaddefaultcerts': True,
        # List of 2-tuple of (hash algorithm, hash).
        'certfingerprints': [],
        # Path to file containing concatenated CA certs. Used by
        # SSLContext.load_verify_locations().
        'cafile': None,
        # Whether certificate verification should be disabled.
        'disablecertverification': False,
        # Whether the legacy [hostfingerprints] section has data for this host.
        'legacyfingerprint': False,
        # PROTOCOL_* constant to use for SSLContext.__init__.
        'protocol': None,
        # String representation of minimum protocol to be used for UI
        # presentation.
        'protocolui': None,
        # ssl.CERT_* constant used by SSLContext.verify_mode.
        'verifymode': None,
        # Defines extra ssl.OP* bitwise options to set.
        'ctxoptions': None,
        # OpenSSL Cipher List to use (instead of default).
        'ciphers': None,
    }

    # Allow minimum TLS protocol to be specified in the config.
    def validateprotocol(protocol, key):
        if protocol not in configprotocols:
            raise error.Abort(
                _('unsupported protocol from hostsecurity.%s: %s') %
                (key, protocol),
                hint=_('valid protocols: %s') %
                     ' '.join(sorted(configprotocols)))

    # We default to TLS 1.1+ where we can because TLS 1.0 has known
    # vulnerabilities (like BEAST and POODLE). We allow users to downgrade to
    # TLS 1.0+ via config options in case a legacy server is encountered.
    if 'tls1.1' in supportedprotocols:
        defaultprotocol = 'tls1.1'
    else:
        # Let people know they are borderline secure.
        # We don't document this config option because we want people to see
        # the bold warnings on the web site.
        # internal config: hostsecurity.disabletls10warning
        if not ui.configbool('hostsecurity', 'disabletls10warning'):
            ui.warn(_('warning: connecting to %s using legacy security '
                      'technology (TLS 1.0); see '
                      'https://mercurial-scm.org/wiki/SecureConnections for '
                      'more info\n') % hostname)
        defaultprotocol = 'tls1.0'

    key = 'minimumprotocol'
    protocol = ui.config('hostsecurity', key, defaultprotocol)
    validateprotocol(protocol, key)

    key = '%s:minimumprotocol' % hostname
    protocol = ui.config('hostsecurity', key, protocol)
    validateprotocol(protocol, key)

    # If --insecure is used, we allow the use of TLS 1.0 despite config options.
    # We always print a "connection security to %s is disabled..." message when
    # --insecure is used. So no need to print anything more here.
    if ui.insecureconnections:
        protocol = 'tls1.0'

    s['protocol'], s['ctxoptions'], s['protocolui'] = protocolsettings(protocol)

    ciphers = ui.config('hostsecurity', 'ciphers')
    ciphers = ui.config('hostsecurity', '%s:ciphers' % hostname, ciphers)
    s['ciphers'] = ciphers

    # Look for fingerprints in [hostsecurity] section. Value is a list
    # of <alg>:<fingerprint> strings.
    fingerprints = ui.configlist('hostsecurity', '%s:fingerprints' % hostname,
                                 [])
    for fingerprint in fingerprints:
        if not (fingerprint.startswith(('sha1:', 'sha256:', 'sha512:'))):
            raise error.Abort(_('invalid fingerprint for %s: %s') % (
                                hostname, fingerprint),
                              hint=_('must begin with "sha1:", "sha256:", '
                                     'or "sha512:"'))

        alg, fingerprint = fingerprint.split(':', 1)
        fingerprint = fingerprint.replace(':', '').lower()
        s['certfingerprints'].append((alg, fingerprint))

    # Fingerprints from [hostfingerprints] are always SHA-1.
    for fingerprint in ui.configlist('hostfingerprints', hostname, []):
        fingerprint = fingerprint.replace(':', '').lower()
        s['certfingerprints'].append(('sha1', fingerprint))
        s['legacyfingerprint'] = True

    # If a host cert fingerprint is defined, it is the only thing that
    # matters. No need to validate CA certs.
    if s['certfingerprints']:
        s['verifymode'] = ssl.CERT_NONE
        s['allowloaddefaultcerts'] = False

    # If --insecure is used, don't take CAs into consideration.
    elif ui.insecureconnections:
        s['disablecertverification'] = True
        s['verifymode'] = ssl.CERT_NONE
        s['allowloaddefaultcerts'] = False

    if ui.configbool('devel', 'disableloaddefaultcerts'):
        s['allowloaddefaultcerts'] = False

    # If both fingerprints and a per-host ca file are specified, issue a warning
    # because users should not be surprised about what security is or isn't
    # being performed.
    cafile = ui.config('hostsecurity', '%s:verifycertsfile' % hostname)
    if s['certfingerprints'] and cafile:
        ui.warn(_('(hostsecurity.%s:verifycertsfile ignored when host '
                  'fingerprints defined; using host fingerprints for '
                  'verification)\n') % hostname)

    # Try to hook up CA certificate validation unless something above
    # makes it not necessary.
    if s['verifymode'] is None:
        # Look at per-host ca file first.
        if cafile:
            cafile = util.expandpath(cafile)
            if not os.path.exists(cafile):
                raise error.Abort(_('path specified by %s does not exist: %s') %
                                  ('hostsecurity.%s:verifycertsfile' % hostname,
                                   cafile))
            s['cafile'] = cafile
        else:
            # Find global certificates file in config.
            cafile = ui.config('web', 'cacerts')

            if cafile:
                cafile = util.expandpath(cafile)
                if not os.path.exists(cafile):
                    raise error.Abort(_('could not find web.cacerts: %s') %
                                      cafile)
            elif s['allowloaddefaultcerts']:
                # CAs not defined in config. Try to find system bundles.
                cafile = _defaultcacerts(ui)
                if cafile:
                    ui.debug('using %s for CA file\n' % cafile)

            s['cafile'] = cafile

        # Require certificate validation if CA certs are being loaded and
        # verification hasn't been disabled above.
        if cafile or (_canloaddefaultcerts and s['allowloaddefaultcerts']):
            s['verifymode'] = ssl.CERT_REQUIRED
        else:
            # At this point we don't have a fingerprint, aren't being
            # explicitly insecure, and can't load CA certs. Connecting
            # is insecure. We allow the connection and abort during
            # validation (once we have the fingerprint to print to the
            # user).
            s['verifymode'] = ssl.CERT_NONE

    assert s['protocol'] is not None
    assert s['ctxoptions'] is not None
    assert s['verifymode'] is not None

    return s

def protocolsettings(protocol):
    """Resolve the protocol for a config value.

    Returns a 3-tuple of (protocol, options, ui value) where the first
    2 items are values used by SSLContext and the last is a string value
    of the ``minimumprotocol`` config option equivalent.
    """
    if protocol not in configprotocols:
        raise ValueError('protocol value not supported: %s' % protocol)

    # Despite its name, PROTOCOL_SSLv23 selects the highest protocol
    # that both ends support, including TLS protocols. On legacy stacks,
    # the highest it likely goes is TLS 1.0. On modern stacks, it can
    # support TLS 1.2.
    #
    # The PROTOCOL_TLSv* constants select a specific TLS version
    # only (as opposed to multiple versions). So the method for
    # supporting multiple TLS versions is to use PROTOCOL_SSLv23 and
    # disable protocols via SSLContext.options and OP_NO_* constants.
    # However, SSLContext.options doesn't work unless we have the
    # full/real SSLContext available to us.
    if supportedprotocols == set(['tls1.0']):
        if protocol != 'tls1.0':
            raise error.Abort(_('current Python does not support protocol '
                                'setting %s') % protocol,
                              hint=_('upgrade Python or disable setting since '
                                     'only TLS 1.0 is supported'))

        return ssl.PROTOCOL_TLSv1, 0, 'tls1.0'

    # WARNING: returned options don't work unless the modern ssl module
    # is available. Be careful when adding options here.

    # SSLv2 and SSLv3 are broken. We ban them outright.
    options = ssl.OP_NO_SSLv2 | ssl.OP_NO_SSLv3

    if protocol == 'tls1.0':
        # Defaults above are to use TLS 1.0+
        pass
    elif protocol == 'tls1.1':
        options |= ssl.OP_NO_TLSv1
    elif protocol == 'tls1.2':
        options |= ssl.OP_NO_TLSv1 | ssl.OP_NO_TLSv1_1
    else:
        raise error.Abort(_('this should not happen'))

    # Prevent CRIME.
    # There is no guarantee this attribute is defined on the module.
    options |= getattr(ssl, 'OP_NO_COMPRESSION', 0)

    return ssl.PROTOCOL_SSLv23, options, protocol

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
        raise error.Abort(_('serverhostname argument is required'))

    settings = _hostsettings(ui, serverhostname)

    # We can't use ssl.create_default_context() because it calls
    # load_default_certs() unless CA arguments are passed to it. We want to
    # have explicit control over CA loading because implicitly loading
    # CAs may undermine the user's intent. For example, a user may define a CA
    # bundle with a specific CA cert removed. If the system/default CA bundle
    # is loaded and contains that removed CA, you've just undone the user's
    # choice.
    sslcontext = SSLContext(settings['protocol'])

    # This is a no-op unless using modern ssl.
    sslcontext.options |= settings['ctxoptions']

    # This still works on our fake SSLContext.
    sslcontext.verify_mode = settings['verifymode']

    if settings['ciphers']:
        try:
            sslcontext.set_ciphers(settings['ciphers'])
        except ssl.SSLError as e:
            raise error.Abort(_('could not set ciphers: %s') % e.args[0],
                              hint=_('change cipher string (%s) in config') %
                                   settings['ciphers'])

    if certfile is not None:
        def password():
            f = keyfile or certfile
            return ui.getpass(_('passphrase for %s: ') % f, '')
        sslcontext.load_cert_chain(certfile, keyfile, password)

    if settings['cafile'] is not None:
        try:
            sslcontext.load_verify_locations(cafile=settings['cafile'])
        except ssl.SSLError as e:
            if len(e.args) == 1: # pypy has different SSLError args
                msg = e.args[0]
            else:
                msg = e.args[1]
            raise error.Abort(_('error loading CA file %s: %s') % (
                              settings['cafile'], msg),
                              hint=_('file is empty or malformed?'))
        caloaded = True
    elif settings['allowloaddefaultcerts']:
        # This is a no-op on old Python.
        sslcontext.load_default_certs()
        caloaded = True
    else:
        caloaded = False

    try:
        sslsocket = sslcontext.wrap_socket(sock, server_hostname=serverhostname)
    except ssl.SSLError as e:
        # If we're doing certificate verification and no CA certs are loaded,
        # that is almost certainly the reason why verification failed. Provide
        # a hint to the user.
        # Only modern ssl module exposes SSLContext.get_ca_certs() so we can
        # only show this warning if modern ssl is available.
        # The exception handler is here because of
        # https://bugs.python.org/issue20916.
        try:
            if (caloaded and settings['verifymode'] == ssl.CERT_REQUIRED and
                modernssl and not sslcontext.get_ca_certs()):
                ui.warn(_('(an attempt was made to load CA certificates but '
                          'none were loaded; see '
                          'https://mercurial-scm.org/wiki/SecureConnections '
                          'for how to configure Mercurial to avoid this '
                          'error)\n'))
        except ssl.SSLError:
            pass
        # Try to print more helpful error messages for known failures.
        if util.safehasattr(e, 'reason'):
            # This error occurs when the client and server don't share a
            # common/supported SSL/TLS protocol. We've disabled SSLv2 and SSLv3
            # outright. Hopefully the reason for this error is that we require
            # TLS 1.1+ and the server only supports TLS 1.0. Whatever the
            # reason, try to emit an actionable warning.
            if e.reason == 'UNSUPPORTED_PROTOCOL':
                # We attempted TLS 1.0+.
                if settings['protocolui'] == 'tls1.0':
                    # We support more than just TLS 1.0+. If this happens,
                    # the likely scenario is either the client or the server
                    # is really old. (e.g. server doesn't support TLS 1.0+ or
                    # client doesn't support modern TLS versions introduced
                    # several years from when this comment was written).
                    if supportedprotocols != set(['tls1.0']):
                        ui.warn(_(
                            '(could not communicate with %s using security '
                            'protocols %s; if you are using a modern Mercurial '
                            'version, consider contacting the operator of this '
                            'server; see '
                            'https://mercurial-scm.org/wiki/SecureConnections '
                            'for more info)\n') % (
                                serverhostname,
                                ', '.join(sorted(supportedprotocols))))
                    else:
                        ui.warn(_(
                            '(could not communicate with %s using TLS 1.0; the '
                            'likely cause of this is the server no longer '
                            'supports TLS 1.0 because it has known security '
                            'vulnerabilities; see '
                            'https://mercurial-scm.org/wiki/SecureConnections '
                            'for more info)\n') % serverhostname)
                else:
                    # We attempted TLS 1.1+. We can only get here if the client
                    # supports the configured protocol. So the likely reason is
                    # the client wants better security than the server can
                    # offer.
                    ui.warn(_(
                        '(could not negotiate a common security protocol (%s+) '
                        'with %s; the likely cause is Mercurial is configured '
                        'to be more secure than the server can support)\n') % (
                        settings['protocolui'], serverhostname))
                    ui.warn(_('(consider contacting the operator of this '
                              'server and ask them to support modern TLS '
                              'protocol versions; or, set '
                              'hostsecurity.%s:minimumprotocol=tls1.0 to allow '
                              'use of legacy, less secure protocols when '
                              'communicating with this server)\n') %
                            serverhostname)
                    ui.warn(_(
                        '(see https://mercurial-scm.org/wiki/SecureConnections '
                        'for more info)\n'))
        raise

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

def wrapserversocket(sock, ui, certfile=None, keyfile=None, cafile=None,
                     requireclientcert=False):
    """Wrap a socket for use by servers.

    ``certfile`` and ``keyfile`` specify the files containing the certificate's
    public and private keys, respectively. Both keys can be defined in the same
    file via ``certfile`` (the private key must come first in the file).

    ``cafile`` defines the path to certificate authorities.

    ``requireclientcert`` specifies whether to require client certificates.

    Typically ``cafile`` is only defined if ``requireclientcert`` is true.
    """
    protocol, options, _protocolui = protocolsettings('tls1.0')

    # This config option is intended for use in tests only. It is a giant
    # footgun to kill security. Don't define it.
    exactprotocol = ui.config('devel', 'serverexactprotocol')
    if exactprotocol == 'tls1.0':
        protocol = ssl.PROTOCOL_TLSv1
    elif exactprotocol == 'tls1.1':
        if 'tls1.1' not in supportedprotocols:
            raise error.Abort(_('TLS 1.1 not supported by this Python'))
        protocol = ssl.PROTOCOL_TLSv1_1
    elif exactprotocol == 'tls1.2':
        if 'tls1.2' not in supportedprotocols:
            raise error.Abort(_('TLS 1.2 not supported by this Python'))
        protocol = ssl.PROTOCOL_TLSv1_2
    elif exactprotocol:
        raise error.Abort(_('invalid value for serverexactprotocol: %s') %
                          exactprotocol)

    if modernssl:
        # We /could/ use create_default_context() here since it doesn't load
        # CAs when configured for client auth. However, it is hard-coded to
        # use ssl.PROTOCOL_SSLv23 which may not be appropriate here.
        sslcontext = SSLContext(protocol)
        sslcontext.options |= options

        # Improve forward secrecy.
        sslcontext.options |= getattr(ssl, 'OP_SINGLE_DH_USE', 0)
        sslcontext.options |= getattr(ssl, 'OP_SINGLE_ECDH_USE', 0)

        # Use the list of more secure ciphers if found in the ssl module.
        if util.safehasattr(ssl, '_RESTRICTED_SERVER_CIPHERS'):
            sslcontext.options |= getattr(ssl, 'OP_CIPHER_SERVER_PREFERENCE', 0)
            sslcontext.set_ciphers(ssl._RESTRICTED_SERVER_CIPHERS)
    else:
        sslcontext = SSLContext(ssl.PROTOCOL_TLSv1)

    if requireclientcert:
        sslcontext.verify_mode = ssl.CERT_REQUIRED
    else:
        sslcontext.verify_mode = ssl.CERT_NONE

    if certfile or keyfile:
        sslcontext.load_cert_chain(certfile=certfile, keyfile=keyfile)

    if cafile:
        sslcontext.load_verify_locations(cafile=cafile)

    return sslcontext.wrap_socket(sock, server_side=True)

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
                return e.args[0]

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
                        return e.args[0]

                    dnsnames.append(value)

    if len(dnsnames) > 1:
        return _('certificate is for %s') % ', '.join(dnsnames)
    elif len(dnsnames) == 1:
        return _('certificate is for %s') % dnsnames[0]
    else:
        return _('no commonName or subjectAltName found in certificate')

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

_systemcacertpaths = [
    # RHEL, CentOS, and Fedora
    '/etc/pki/tls/certs/ca-bundle.trust.crt',
    # Debian, Ubuntu, Gentoo
    '/etc/ssl/certs/ca-certificates.crt',
]

def _defaultcacerts(ui):
    """return path to default CA certificates or None.

    It is assumed this function is called when the returned certificates
    file will actually be used to validate connections. Therefore this
    function may print warnings or debug messages assuming this usage.

    We don't print a message when the Python is able to load default
    CA certs because this scenario is detected at socket connect time.
    """
    # The "certifi" Python package provides certificates. If it is installed,
    # assume the user intends it to be used and use it.
    try:
        import certifi
        certs = certifi.where()
        ui.debug('using ca certificates from certifi\n')
        return certs
    except ImportError:
        pass

    # On Windows, only the modern ssl module is capable of loading the system
    # CA certificates. If we're not capable of doing that, emit a warning
    # because we'll get a certificate verification error later and the lack
    # of loaded CA certificates will be the reason why.
    # Assertion: this code is only called if certificates are being verified.
    if os.name == 'nt':
        if not _canloaddefaultcerts:
            ui.warn(_('(unable to load Windows CA certificates; see '
                      'https://mercurial-scm.org/wiki/SecureConnections for '
                      'how to configure Mercurial to avoid this message)\n'))

        return None

    # Apple's OpenSSL has patches that allow a specially constructed certificate
    # to load the system CA store. If we're running on Apple Python, use this
    # trick.
    if _plainapplepython():
        dummycert = os.path.join(os.path.dirname(__file__), 'dummycert.pem')
        if os.path.exists(dummycert):
            return dummycert

    # The Apple OpenSSL trick isn't available to us. If Python isn't able to
    # load system certs, we're out of luck.
    if sys.platform == 'darwin':
        # FUTURE Consider looking for Homebrew or MacPorts installed certs
        # files. Also consider exporting the keychain certs to a file during
        # Mercurial install.
        if not _canloaddefaultcerts:
            ui.warn(_('(unable to load CA certificates; see '
                      'https://mercurial-scm.org/wiki/SecureConnections for '
                      'how to configure Mercurial to avoid this message)\n'))
        return None

    # / is writable on Windows. Out of an abundance of caution make sure
    # we're not on Windows because paths from _systemcacerts could be installed
    # by non-admin users.
    assert os.name != 'nt'

    # Try to find CA certificates in well-known locations. We print a warning
    # when using a found file because we don't want too much silent magic
    # for security settings. The expectation is that proper Mercurial
    # installs will have the CA certs path defined at install time and the
    # installer/packager will make an appropriate decision on the user's
    # behalf. We only get here and perform this setting as a feature of
    # last resort.
    if not _canloaddefaultcerts:
        for path in _systemcacertpaths:
            if os.path.isfile(path):
                ui.warn(_('(using CA certificates from %s; if you see this '
                          'message, your Mercurial install is not properly '
                          'configured; see '
                          'https://mercurial-scm.org/wiki/SecureConnections '
                          'for how to configure Mercurial to avoid this '
                          'message)\n') % path)
                return path

        ui.warn(_('(unable to load CA certificates; see '
                  'https://mercurial-scm.org/wiki/SecureConnections for '
                  'how to configure Mercurial to avoid this message)\n'))

    return None

def validatesocket(sock):
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

    if settings['disablecertverification']:
        # We don't print the certificate fingerprint because it shouldn't
        # be necessary: if the user requested certificate verification be
        # disabled, they presumably already saw a message about the inability
        # to verify the certificate and this message would have printed the
        # fingerprint. So printing the fingerprint here adds little to no
        # value.
        ui.warn(_('warning: connection security to %s is disabled per current '
                  'settings; communication is susceptible to eavesdropping '
                  'and tampering\n') % host)
        return

    # If a certificate fingerprint is pinned, use it and only it to
    # validate the remote cert.
    peerfingerprints = {
        'sha1': hashlib.sha1(peercert).hexdigest(),
        'sha256': hashlib.sha256(peercert).hexdigest(),
        'sha512': hashlib.sha512(peercert).hexdigest(),
    }

    def fmtfingerprint(s):
        return ':'.join([s[x:x + 2] for x in range(0, len(s), 2)])

    nicefingerprint = 'sha256:%s' % fmtfingerprint(peerfingerprints['sha256'])

    if settings['certfingerprints']:
        for hash, fingerprint in settings['certfingerprints']:
            if peerfingerprints[hash].lower() == fingerprint:
                ui.debug('%s certificate matched fingerprint %s:%s\n' %
                         (host, hash, fmtfingerprint(fingerprint)))
                return

        # Pinned fingerprint didn't match. This is a fatal error.
        if settings['legacyfingerprint']:
            section = 'hostfingerprint'
            nice = fmtfingerprint(peerfingerprints['sha1'])
        else:
            section = 'hostsecurity'
            nice = '%s:%s' % (hash, fmtfingerprint(peerfingerprints[hash]))
        raise error.Abort(_('certificate for %s has unexpected '
                            'fingerprint %s') % (host, nice),
                          hint=_('check %s configuration') % section)

    # Security is enabled but no CAs are loaded. We can't establish trust
    # for the cert so abort.
    if not sock._hgstate['caloaded']:
        raise error.Abort(
            _('unable to verify security of %s (no loaded CA certificates); '
              'refusing to connect') % host,
            hint=_('see https://mercurial-scm.org/wiki/SecureConnections for '
                   'how to configure Mercurial to avoid this error or set '
                   'hostsecurity.%s:fingerprints=%s to trust this server') %
                   (host, nicefingerprint))

    msg = _verifycert(peercert2, host)
    if msg:
        raise error.Abort(_('%s certificate error: %s') % (host, msg),
                         hint=_('set hostsecurity.%s:certfingerprints=%s '
                                'config setting or use --insecure to connect '
                                'insecurely') %
                              (host, nicefingerprint))
