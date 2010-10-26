import sys

def check(a, b):
    if a != b:
        print (a, b)

def cert(cn):
    return dict(subject=((('commonName', cn),),))

from mercurial.url import _verifycert

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

# Avoid some pitfalls
check(_verifycert(cert('*.foo'), 'foo'),
      'certificate is for *.foo')
check(_verifycert(cert('*o'), 'foo'),
      'certificate is for *o')

check(_verifycert({'subject': ()},
                  'example.com'),
      'no commonName found in certificate')
check(_verifycert(None, 'example.com'),
      'no certificate received')
