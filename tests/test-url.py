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

# Test subjectAltName
san_cert = {'subject': ((('commonName', 'example.com'),),),
            'subjectAltName': (('DNS', '*.example.net'),
                               ('DNS', 'example.net'))}
check(_verifycert(san_cert, 'example.net'),
      None)
check(_verifycert(san_cert, 'foo.example.net'),
      None)
# subject is only checked when subjectAltName is empty
check(_verifycert(san_cert, 'example.com'),
      'certificate is for *.example.net, example.net')

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
