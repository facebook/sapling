from __future__ import absolute_import, print_function

import doctest
import os

def check(a, b):
    if a != b:
        print((a, b))

def cert(cn):
    return {'subject': ((('commonName', cn),),)}

from mercurial import (
    sslutil,
)

_verifycert = sslutil._verifycert
# Test non-wildcard certificates
check(_verifycert(cert('example.com'), 'example.com'),
      None)
check(_verifycert(cert('example.com'), 'www.example.com'),
      'certificate is for example.com')
check(_verifycert(cert('www.example.com'), 'example.com'),
      'certificate is for www.example.com')

# Test wildcard certificates
check(_verifycert(cert('*.example.com'), 'www.example.com'),
      None)
check(_verifycert(cert('*.example.com'), 'example.com'),
      'certificate is for *.example.com')
check(_verifycert(cert('*.example.com'), 'w.w.example.com'),
      'certificate is for *.example.com')

# Test subjectAltName
san_cert = {'subject': ((('commonName', 'example.com'),),),
            'subjectAltName': (('DNS', '*.example.net'),
                               ('DNS', 'example.net'))}
check(_verifycert(san_cert, 'example.net'),
      None)
check(_verifycert(san_cert, 'foo.example.net'),
      None)
# no fallback to subject commonName when subjectAltName has DNS
check(_verifycert(san_cert, 'example.com'),
      'certificate is for *.example.net, example.net')
# fallback to subject commonName when no DNS in subjectAltName
san_cert = {'subject': ((('commonName', 'example.com'),),),
            'subjectAltName': (('IP Address', '8.8.8.8'),)}
check(_verifycert(san_cert, 'example.com'), None)

# Avoid some pitfalls
check(_verifycert(cert('*.foo'), 'foo'),
      'certificate is for *.foo')
check(_verifycert(cert('*o'), 'foo'),
      'certificate is for *o')

check(_verifycert({'subject': ()},
                  'example.com'),
      'no commonName or subjectAltName found in certificate')
check(_verifycert(None, 'example.com'),
      'no certificate received')

# Unicode (IDN) certname isn't supported
check(_verifycert(cert(u'\u4f8b.jp'), 'example.jp'),
      'IDN in certificate not supported')

def test_url():
    """
    >>> from mercurial.util import url

    This tests for edge cases in url.URL's parsing algorithm. Most of
    these aren't useful for documentation purposes, so they aren't
    part of the class's doc tests.

    Query strings and fragments:

    >>> url('http://host/a?b#c')
    <url scheme: 'http', host: 'host', path: 'a', query: 'b', fragment: 'c'>
    >>> url('http://host/a?')
    <url scheme: 'http', host: 'host', path: 'a'>
    >>> url('http://host/a#b#c')
    <url scheme: 'http', host: 'host', path: 'a', fragment: 'b#c'>
    >>> url('http://host/a#b?c')
    <url scheme: 'http', host: 'host', path: 'a', fragment: 'b?c'>
    >>> url('http://host/?a#b')
    <url scheme: 'http', host: 'host', path: '', query: 'a', fragment: 'b'>
    >>> url('http://host/?a#b', parsequery=False)
    <url scheme: 'http', host: 'host', path: '?a', fragment: 'b'>
    >>> url('http://host/?a#b', parsefragment=False)
    <url scheme: 'http', host: 'host', path: '', query: 'a#b'>
    >>> url('http://host/?a#b', parsequery=False, parsefragment=False)
    <url scheme: 'http', host: 'host', path: '?a#b'>

    IPv6 addresses:

    >>> url('ldap://[2001:db8::7]/c=GB?objectClass?one')
    <url scheme: 'ldap', host: '[2001:db8::7]', path: 'c=GB',
         query: 'objectClass?one'>
    >>> url('ldap://joe:xxx@[2001:db8::7]:80/c=GB?objectClass?one')
    <url scheme: 'ldap', user: 'joe', passwd: 'xxx', host: '[2001:db8::7]',
         port: '80', path: 'c=GB', query: 'objectClass?one'>

    Missing scheme, host, etc.:

    >>> url('://192.0.2.16:80/')
    <url path: '://192.0.2.16:80/'>
    >>> url('https://mercurial-scm.org')
    <url scheme: 'https', host: 'mercurial-scm.org'>
    >>> url('/foo')
    <url path: '/foo'>
    >>> url('bundle:/foo')
    <url scheme: 'bundle', path: '/foo'>
    >>> url('a?b#c')
    <url path: 'a?b', fragment: 'c'>
    >>> url('http://x.com?arg=/foo')
    <url scheme: 'http', host: 'x.com', query: 'arg=/foo'>
    >>> url('http://joe:xxx@/foo')
    <url scheme: 'http', user: 'joe', passwd: 'xxx', path: 'foo'>

    Just a scheme and a path:

    >>> url('mailto:John.Doe@example.com')
    <url scheme: 'mailto', path: 'John.Doe@example.com'>
    >>> url('a:b:c:d')
    <url path: 'a:b:c:d'>
    >>> url('aa:bb:cc:dd')
    <url scheme: 'aa', path: 'bb:cc:dd'>

    SSH examples:

    >>> url('ssh://joe@host//home/joe')
    <url scheme: 'ssh', user: 'joe', host: 'host', path: '/home/joe'>
    >>> url('ssh://joe:xxx@host/src')
    <url scheme: 'ssh', user: 'joe', passwd: 'xxx', host: 'host', path: 'src'>
    >>> url('ssh://joe:xxx@host')
    <url scheme: 'ssh', user: 'joe', passwd: 'xxx', host: 'host'>
    >>> url('ssh://joe@host')
    <url scheme: 'ssh', user: 'joe', host: 'host'>
    >>> url('ssh://host')
    <url scheme: 'ssh', host: 'host'>
    >>> url('ssh://')
    <url scheme: 'ssh'>
    >>> url('ssh:')
    <url scheme: 'ssh'>

    Non-numeric port:

    >>> url('http://example.com:dd')
    <url scheme: 'http', host: 'example.com', port: 'dd'>
    >>> url('ssh://joe:xxx@host:ssh/foo')
    <url scheme: 'ssh', user: 'joe', passwd: 'xxx', host: 'host', port: 'ssh',
         path: 'foo'>

    Bad authentication credentials:

    >>> url('http://joe@joeville:123@4:@host/a?b#c')
    <url scheme: 'http', user: 'joe@joeville', passwd: '123@4:',
         host: 'host', path: 'a', query: 'b', fragment: 'c'>
    >>> url('http://!*#?/@!*#?/:@host/a?b#c')
    <url scheme: 'http', host: '!*', fragment: '?/@!*#?/:@host/a?b#c'>
    >>> url('http://!*#?@!*#?:@host/a?b#c')
    <url scheme: 'http', host: '!*', fragment: '?@!*#?:@host/a?b#c'>
    >>> url('http://!*@:!*@@host/a?b#c')
    <url scheme: 'http', user: '!*@', passwd: '!*@', host: 'host',
         path: 'a', query: 'b', fragment: 'c'>

    File paths:

    >>> url('a/b/c/d.g.f')
    <url path: 'a/b/c/d.g.f'>
    >>> url('/x///z/y/')
    <url path: '/x///z/y/'>
    >>> url('/foo:bar')
    <url path: '/foo:bar'>
    >>> url('\\\\foo:bar')
    <url path: '\\\\foo:bar'>
    >>> url('./foo:bar')
    <url path: './foo:bar'>

    Non-localhost file URL:

    >>> u = url('file://mercurial-scm.org/foo')
    Traceback (most recent call last):
      File "<stdin>", line 1, in ?
    Abort: file:// URLs can only refer to localhost

    Empty URL:

    >>> u = url('')
    >>> u
    <url path: ''>
    >>> str(u)
    ''

    Empty path with query string:

    >>> str(url('http://foo/?bar'))
    'http://foo/?bar'

    Invalid path:

    >>> u = url('http://foo/bar')
    >>> u.path = 'bar'
    >>> str(u)
    'http://foo/bar'

    >>> u = url('file:/foo/bar/baz')
    >>> u
    <url scheme: 'file', path: '/foo/bar/baz'>
    >>> str(u)
    'file:///foo/bar/baz'
    >>> u.localpath()
    '/foo/bar/baz'

    >>> u = url('file:///foo/bar/baz')
    >>> u
    <url scheme: 'file', path: '/foo/bar/baz'>
    >>> str(u)
    'file:///foo/bar/baz'
    >>> u.localpath()
    '/foo/bar/baz'

    >>> u = url('file:///f:oo/bar/baz')
    >>> u
    <url scheme: 'file', path: 'f:oo/bar/baz'>
    >>> str(u)
    'file:///f:oo/bar/baz'
    >>> u.localpath()
    'f:oo/bar/baz'

    >>> u = url('file://localhost/f:oo/bar/baz')
    >>> u
    <url scheme: 'file', host: 'localhost', path: 'f:oo/bar/baz'>
    >>> str(u)
    'file://localhost/f:oo/bar/baz'
    >>> u.localpath()
    'f:oo/bar/baz'

    >>> u = url('file:foo/bar/baz')
    >>> u
    <url scheme: 'file', path: 'foo/bar/baz'>
    >>> str(u)
    'file:foo/bar/baz'
    >>> u.localpath()
    'foo/bar/baz'
    """

if 'TERM' in os.environ:
    del os.environ['TERM']

doctest.testmod(optionflags=doctest.NORMALIZE_WHITESPACE)
