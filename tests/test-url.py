#!/usr/bin/env python
import sys
try:
    import ssl
except ImportError:
    sys.exit(80)

def check(a, b):
    if a != b:
        print (a, b)

from mercurial.url import _verifycert

# Test non-wildcard certificates        
check(_verifycert({'subject': ((('commonName', 'example.com'),),)}, 'example.com'),
    None)
check(_verifycert({'subject': ((('commonName', 'example.com'),),)}, 'www.example.com'),
    'certificate is for example.com')
check(_verifycert({'subject': ((('commonName', 'www.example.com'),),)}, 'example.com'),
    'certificate is for www.example.com')

# Test wildcard certificates
check(_verifycert({'subject': ((('commonName', '*.example.com'),),)}, 'www.example.com'),
    None)
check(_verifycert({'subject': ((('commonName', '*.example.com'),),)}, 'example.com'),
    'certificate is for *.example.com')
check(_verifycert({'subject': ((('commonName', '*.example.com'),),)}, 'w.w.example.com'),
    'certificate is for *.example.com')

# Avoid some pitfalls
check(_verifycert({'subject': ((('commonName', '*.foo'),),)}, 'foo'),
    'certificate is for *.foo')
check(_verifycert({'subject': ((('commonName', '*o'),),)}, 'foo'),
    'certificate is for *o')

import time
lastyear = time.gmtime().tm_year - 1
nextyear = time.gmtime().tm_year + 1
check(_verifycert({'notAfter': 'May  9 00:00:00 %s GMT' % lastyear}, 'example.com'),
    'certificate expired May  9 00:00:00 %s GMT' % lastyear)
check(_verifycert({'notBefore': 'May  9 00:00:00 %s GMT' % nextyear}, 'example.com'),
    'certificate not valid before May  9 00:00:00 %s GMT' % nextyear)
check(_verifycert({'notAfter': 'Sep 29 15:29:48 %s GMT' % nextyear, 'subject': ()}, 'example.com'),
    'no commonName found in certificate')
check(_verifycert(None, 'example.com'),
    'no certificate received')
