#require serve ssl

Proper https client requires the built-in ssl from Python 2.6.

Certificates created with:
 printf '.\n.\n.\n.\n.\nlocalhost\nhg@localhost\n' | \
 openssl req -newkey rsa:512 -keyout priv.pem -nodes -x509 -days 9000 -out pub.pem
Can be dumped with:
 openssl x509 -in pub.pem -text

  $ cat << EOT > priv.pem
  > -----BEGIN PRIVATE KEY-----
  > MIIBVAIBADANBgkqhkiG9w0BAQEFAASCAT4wggE6AgEAAkEApjCWeYGrIa/Vo7LH
  > aRF8ou0tbgHKE33Use/whCnKEUm34rDaXQd4lxxX6aDWg06n9tiVStAKTgQAHJY8
  > j/xgSwIDAQABAkBxHC6+Qlf0VJXGlb6NL16yEVVTQxqDS6hA9zqu6TZjrr0YMfzc
  > EGNIiZGt7HCBL0zO+cPDg/LeCZc6HQhf0KrhAiEAzlJq4hWWzvguWFIJWSoBeBUG
  > MF1ACazQO7PYE8M0qfECIQDONHHP0SKZzz/ZwBZcAveC5K61f/v9hONFwbeYulzR
  > +wIgc9SvbtgB/5Yzpp//4ZAEnR7oh5SClCvyB+KSx52K3nECICbhQphhoXmI10wy
  > aMTellaq0bpNMHFDziqH9RsqAHhjAiEAgYGxfzkftt5IUUn/iFK89aaIpyrpuaAh
  > HY8gUVkVRVs=
  > -----END PRIVATE KEY-----
  > EOT

  $ cat << EOT > pub.pem
  > -----BEGIN CERTIFICATE-----
  > MIIBqzCCAVWgAwIBAgIJANAXFFyWjGnRMA0GCSqGSIb3DQEBBQUAMDExEjAQBgNV
  > BAMMCWxvY2FsaG9zdDEbMBkGCSqGSIb3DQEJARYMaGdAbG9jYWxob3N0MB4XDTEw
  > MTAxNDIwMzAxNFoXDTM1MDYwNTIwMzAxNFowMTESMBAGA1UEAwwJbG9jYWxob3N0
  > MRswGQYJKoZIhvcNAQkBFgxoZ0Bsb2NhbGhvc3QwXDANBgkqhkiG9w0BAQEFAANL
  > ADBIAkEApjCWeYGrIa/Vo7LHaRF8ou0tbgHKE33Use/whCnKEUm34rDaXQd4lxxX
  > 6aDWg06n9tiVStAKTgQAHJY8j/xgSwIDAQABo1AwTjAdBgNVHQ4EFgQUE6sA+amm
  > r24dGX0kpjxOgO45hzQwHwYDVR0jBBgwFoAUE6sA+ammr24dGX0kpjxOgO45hzQw
  > DAYDVR0TBAUwAwEB/zANBgkqhkiG9w0BAQUFAANBAFArvQFiAZJgQczRsbYlG1xl
  > t+truk37w5B3m3Ick1ntRcQrqs+hf0CO1q6Squ144geYaQ8CDirSR92fICELI1c=
  > -----END CERTIFICATE-----
  > EOT
  $ cat priv.pem pub.pem >> server.pem
  $ PRIV=`pwd`/server.pem

  $ cat << EOT > pub-other.pem
  > -----BEGIN CERTIFICATE-----
  > MIIBqzCCAVWgAwIBAgIJALwZS731c/ORMA0GCSqGSIb3DQEBBQUAMDExEjAQBgNV
  > BAMMCWxvY2FsaG9zdDEbMBkGCSqGSIb3DQEJARYMaGdAbG9jYWxob3N0MB4XDTEw
  > MTAxNDIwNDUxNloXDTM1MDYwNTIwNDUxNlowMTESMBAGA1UEAwwJbG9jYWxob3N0
  > MRswGQYJKoZIhvcNAQkBFgxoZ0Bsb2NhbGhvc3QwXDANBgkqhkiG9w0BAQEFAANL
  > ADBIAkEAsxsapLbHrqqUKuQBxdpK4G3m2LjtyrTSdpzzzFlecxd5yhNP6AyWrufo
  > K4VMGo2xlu9xOo88nDSUNSKPuD09MwIDAQABo1AwTjAdBgNVHQ4EFgQUoIB1iMhN
  > y868rpQ2qk9dHnU6ebswHwYDVR0jBBgwFoAUoIB1iMhNy868rpQ2qk9dHnU6ebsw
  > DAYDVR0TBAUwAwEB/zANBgkqhkiG9w0BAQUFAANBAJ544f125CsE7J2t55PdFaF6
  > bBlNBb91FCywBgSjhBjf+GG3TNPwrPdc3yqeq+hzJiuInqbOBv9abmMyq8Wsoig=
  > -----END CERTIFICATE-----
  > EOT

pub.pem patched with other notBefore / notAfter:

  $ cat << EOT > pub-not-yet.pem
  > -----BEGIN CERTIFICATE-----
  > MIIBqzCCAVWgAwIBAgIJANAXFFyWjGnRMA0GCSqGSIb3DQEBBQUAMDExEjAQBgNVBAMMCWxvY2Fs
  > aG9zdDEbMBkGCSqGSIb3DQEJARYMaGdAbG9jYWxob3N0MB4XDTM1MDYwNTIwMzAxNFoXDTM1MDYw
  > NTIwMzAxNFowMTESMBAGA1UEAwwJbG9jYWxob3N0MRswGQYJKoZIhvcNAQkBFgxoZ0Bsb2NhbGhv
  > c3QwXDANBgkqhkiG9w0BAQEFAANLADBIAkEApjCWeYGrIa/Vo7LHaRF8ou0tbgHKE33Use/whCnK
  > EUm34rDaXQd4lxxX6aDWg06n9tiVStAKTgQAHJY8j/xgSwIDAQABo1AwTjAdBgNVHQ4EFgQUE6sA
  > +ammr24dGX0kpjxOgO45hzQwHwYDVR0jBBgwFoAUE6sA+ammr24dGX0kpjxOgO45hzQwDAYDVR0T
  > BAUwAwEB/zANBgkqhkiG9w0BAQUFAANBAJXV41gWnkgC7jcpPpFRSUSZaxyzrXmD1CIqQf0WgVDb
  > /12E0vR2DuZitgzUYtBaofM81aTtc0a2/YsrmqePGm0=
  > -----END CERTIFICATE-----
  > EOT
  $ cat priv.pem pub-not-yet.pem > server-not-yet.pem

  $ cat << EOT > pub-expired.pem
  > -----BEGIN CERTIFICATE-----
  > MIIBqzCCAVWgAwIBAgIJANAXFFyWjGnRMA0GCSqGSIb3DQEBBQUAMDExEjAQBgNVBAMMCWxvY2Fs
  > aG9zdDEbMBkGCSqGSIb3DQEJARYMaGdAbG9jYWxob3N0MB4XDTEwMTAxNDIwMzAxNFoXDTEwMTAx
  > NDIwMzAxNFowMTESMBAGA1UEAwwJbG9jYWxob3N0MRswGQYJKoZIhvcNAQkBFgxoZ0Bsb2NhbGhv
  > c3QwXDANBgkqhkiG9w0BAQEFAANLADBIAkEApjCWeYGrIa/Vo7LHaRF8ou0tbgHKE33Use/whCnK
  > EUm34rDaXQd4lxxX6aDWg06n9tiVStAKTgQAHJY8j/xgSwIDAQABo1AwTjAdBgNVHQ4EFgQUE6sA
  > +ammr24dGX0kpjxOgO45hzQwHwYDVR0jBBgwFoAUE6sA+ammr24dGX0kpjxOgO45hzQwDAYDVR0T
  > BAUwAwEB/zANBgkqhkiG9w0BAQUFAANBAJfk57DTRf2nUbYaMSlVAARxMNbFGOjQhAUtY400GhKt
  > 2uiKCNGKXVXD3AHWe13yHc5KttzbHQStE5Nm/DlWBWQ=
  > -----END CERTIFICATE-----
  > EOT
  $ cat priv.pem pub-expired.pem > server-expired.pem

Client certificates created with:
 openssl genrsa -aes128 -passout pass:1234 -out client-key.pem 512
 openssl rsa -in client-key.pem -passin pass:1234 -out client-key-decrypted.pem
 printf '.\n.\n.\n.\n.\n.\nhg-client@localhost\n.\n.\n' | \
 openssl req -new -key client-key.pem -passin pass:1234 -out client-csr.pem
 openssl x509 -req -days 9000 -in client-csr.pem -CA pub.pem -CAkey priv.pem \
 -set_serial 01 -out client-cert.pem

  $ cat << EOT > client-key.pem
  > -----BEGIN RSA PRIVATE KEY-----
  > Proc-Type: 4,ENCRYPTED
  > DEK-Info: AES-128-CBC,C8B8F103A61A336FB0716D1C0F8BB2E8
  > 
  > JolMlCFjEW3q3JJjO9z99NJWeJbFgF5DpUOkfSCxH56hxxtZb9x++rBvBZkxX1bF
  > BAIe+iI90+jdCLwxbILWuFcrJUaLC5WmO14XDKYVmr2eW9e4MiCYOlO0Q6a9rDFS
  > jctRCfvubOXFHbBGLH8uKEMpXEkP7Lc60FiIukqjuQEivJjrQirVtZCGwyk3qUi7
  > Eyh4Lo63IKGu8T1Bkmn2kaMvFhu7nC/CQLBjSq0YYI1tmCOkVb/3tPrz8oqgDJp2
  > u7bLS3q0xDNZ52nVrKIoZC/UlRXGlPyzPpa70/jPIdfCbkwDaBpRVXc+62Pj2n5/
  > CnO2xaKwfOG6pDvanBhFD72vuBOkAYlFZPiEku4sc2WlNggsSWCPCIFwzmiHjKIl
  > bWmdoTq3nb7sNfnBbV0OCa7fS1dFwCm4R1NC7ELENu0=
  > -----END RSA PRIVATE KEY-----
  > EOT

  $ cat << EOT > client-key-decrypted.pem
  > -----BEGIN RSA PRIVATE KEY-----
  > MIIBOgIBAAJBAJs4LS3glAYU92bg5kPgRPNW84ewB0fWJfAKccCp1ACHAdZPeaKb
  > FCinVMYKAVbVqBkyrZ/Tyr8aSfMz4xO4+KsCAwEAAQJAeKDr25+Q6jkZHEbkLRP6
  > AfMtR+Ixhk6TJT24sbZKIC2V8KuJTDEvUhLU0CAr1nH79bDqiSsecOiVCr2HHyfT
  > AQIhAM2C5rHbTs9R3PkywFEqq1gU3ztCnpiWglO7/cIkuGBhAiEAwVpMSAf77kop
  > 4h/1kWsgMALQTJNsXd4CEUK4BOxvJIsCIQCbarVAKBQvoT81jfX27AfscsxnKnh5
  > +MjSvkanvdFZwQIgbbcTefwt1LV4trtz2SR0i0nNcOZmo40Kl0jIquKO3qkCIH01
  > mJHzZr3+jQqeIFtr5P+Xqi30DJxgrnEobbJ0KFjY
  > -----END RSA PRIVATE KEY-----
  > EOT

  $ cat << EOT > client-cert.pem
  > -----BEGIN CERTIFICATE-----
  > MIIBPjCB6QIBATANBgkqhkiG9w0BAQsFADAxMRIwEAYDVQQDDAlsb2NhbGhvc3Qx
  > GzAZBgkqhkiG9w0BCQEWDGhnQGxvY2FsaG9zdDAeFw0xNTA1MDcwNjI5NDVaFw0z
  > OTEyMjcwNjI5NDVaMCQxIjAgBgkqhkiG9w0BCQEWE2hnLWNsaWVudEBsb2NhbGhv
  > c3QwXDANBgkqhkiG9w0BAQEFAANLADBIAkEAmzgtLeCUBhT3ZuDmQ+BE81bzh7AH
  > R9Yl8ApxwKnUAIcB1k95opsUKKdUxgoBVtWoGTKtn9PKvxpJ8zPjE7j4qwIDAQAB
  > MA0GCSqGSIb3DQEBCwUAA0EAfBTqBG5pYhuGk+ZnyUufgS+d7Nk/sZAZjNdCAEj/
  > NFPo5fR1jM6jlEWoWbeg298+SkjV7tfO+2nt0otUFkdM6A==
  > -----END CERTIFICATE-----
  > EOT

  $ hg init test
  $ cd test
  $ echo foo>foo
  $ mkdir foo.d foo.d/bAr.hg.d foo.d/baR.d.hg
  $ echo foo>foo.d/foo
  $ echo bar>foo.d/bAr.hg.d/BaR
  $ echo bar>foo.d/baR.d.hg/bAR
  $ hg commit -A -m 1
  adding foo
  adding foo.d/bAr.hg.d/BaR
  adding foo.d/baR.d.hg/bAR
  adding foo.d/foo
  $ hg serve -p $HGPORT -d --pid-file=../hg0.pid --certificate=$PRIV
  $ cat ../hg0.pid >> $DAEMON_PIDS

cacert not found

  $ hg in --config web.cacerts=no-such.pem https://localhost:$HGPORT/
  abort: could not find web.cacerts: no-such.pem
  [255]

Test server address cannot be reused

#if windows
  $ hg serve -p $HGPORT --certificate=$PRIV 2>&1
  abort: cannot start server at ':$HGPORT':
  [255]
#else
  $ hg serve -p $HGPORT --certificate=$PRIV 2>&1
  abort: cannot start server at ':$HGPORT': Address already in use
  [255]
#endif
  $ cd ..

OS X has a dummy CA cert that enables use of the system CA store when using
Apple's OpenSSL. This trick do not work with plain OpenSSL.

  $ DISABLEOSXDUMMYCERT=
#if defaultcacerts
  $ hg clone https://localhost:$HGPORT/ copy-pull
  abort: error: *certificate verify failed* (glob)
  [255]

  $ DISABLEOSXDUMMYCERT="--config=web.cacerts=!"
#endif

clone via pull

  $ hg clone https://localhost:$HGPORT/ copy-pull $DISABLEOSXDUMMYCERT
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg verify -R copy-pull
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 1 changesets, 4 total revisions
  $ cd test
  $ echo bar > bar
  $ hg commit -A -d '1 0' -m 2
  adding bar
  $ cd ..

pull without cacert

  $ cd copy-pull
  $ echo '[hooks]' >> .hg/hgrc
  $ echo "changegroup = printenv.py changegroup" >> .hg/hgrc
  $ hg pull $DISABLEOSXDUMMYCERT
  pulling from https://localhost:$HGPORT/
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  changegroup hook: HG_NODE=5fed3813f7f5e1824344fdc9cf8f63bb662c292d HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=https://localhost:$HGPORT/ (glob)
  (run 'hg update' to get a working copy)
  $ cd ..

cacert configured in local repo

  $ cp copy-pull/.hg/hgrc copy-pull/.hg/hgrc.bu
  $ echo "[web]" >> copy-pull/.hg/hgrc
  $ echo "cacerts=`pwd`/pub.pem" >> copy-pull/.hg/hgrc
  $ hg -R copy-pull pull --traceback
  pulling from https://localhost:$HGPORT/
  searching for changes
  no changes found
  $ mv copy-pull/.hg/hgrc.bu copy-pull/.hg/hgrc

cacert configured globally, also testing expansion of environment
variables in the filename

  $ echo "[web]" >> $HGRCPATH
  $ echo 'cacerts=$P/pub.pem' >> $HGRCPATH
  $ P=`pwd` hg -R copy-pull pull
  pulling from https://localhost:$HGPORT/
  searching for changes
  no changes found
  $ P=`pwd` hg -R copy-pull pull --insecure
  pulling from https://localhost:$HGPORT/
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  searching for changes
  no changes found

cacert mismatch

  $ hg -R copy-pull pull --config web.cacerts=pub.pem https://127.0.0.1:$HGPORT/
  pulling from https://127.0.0.1:$HGPORT/
  abort: 127.0.0.1 certificate error: certificate is for localhost
  (configure hostfingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca or use --insecure to connect insecurely)
  [255]
  $ hg -R copy-pull pull --config web.cacerts=pub.pem https://127.0.0.1:$HGPORT/ --insecure
  pulling from https://127.0.0.1:$HGPORT/
  warning: 127.0.0.1 certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  searching for changes
  no changes found
  $ hg -R copy-pull pull --config web.cacerts=pub-other.pem
  pulling from https://localhost:$HGPORT/
  abort: error: *certificate verify failed* (glob)
  [255]
  $ hg -R copy-pull pull --config web.cacerts=pub-other.pem --insecure
  pulling from https://localhost:$HGPORT/
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  searching for changes
  no changes found

Test server cert which isn't valid yet

  $ hg -R test serve -p $HGPORT1 -d --pid-file=hg1.pid --certificate=server-not-yet.pem
  $ cat hg1.pid >> $DAEMON_PIDS
  $ hg -R copy-pull pull --config web.cacerts=pub-not-yet.pem https://localhost:$HGPORT1/
  pulling from https://localhost:$HGPORT1/
  abort: error: *certificate verify failed* (glob)
  [255]

Test server cert which no longer is valid

  $ hg -R test serve -p $HGPORT2 -d --pid-file=hg2.pid --certificate=server-expired.pem
  $ cat hg2.pid >> $DAEMON_PIDS
  $ hg -R copy-pull pull --config web.cacerts=pub-expired.pem https://localhost:$HGPORT2/
  pulling from https://localhost:$HGPORT2/
  abort: error: *certificate verify failed* (glob)
  [255]

Fingerprints

  $ echo "[hostfingerprints]" >> copy-pull/.hg/hgrc
  $ echo "localhost = 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca" >> copy-pull/.hg/hgrc
  $ echo "127.0.0.1 = 914f1aff87249c09b6859b88b1906d30756491ca" >> copy-pull/.hg/hgrc

- works without cacerts
  $ hg -R copy-pull id https://localhost:$HGPORT/ --config web.cacerts=!
  5fed3813f7f5

- fails when cert doesn't match hostname (port is ignored)
  $ hg -R copy-pull id https://localhost:$HGPORT1/
  abort: certificate for localhost has unexpected fingerprint 28:ff:71:bf:65:31:14:23:ad:62:92:b4:0e:31:99:18:fc:83:e3:9b
  (check hostfingerprint configuration)
  [255]


- ignores that certificate doesn't match hostname
  $ hg -R copy-pull id https://127.0.0.1:$HGPORT/
  5fed3813f7f5

HGPORT1 is reused below for tinyproxy tests. Kill that server.
  $ killdaemons.py hg1.pid

Prepare for connecting through proxy

  $ tinyproxy.py $HGPORT1 localhost >proxy.log </dev/null 2>&1 &
  $ while [ ! -f proxy.pid ]; do sleep 0; done
  $ cat proxy.pid >> $DAEMON_PIDS

  $ echo "[http_proxy]" >> copy-pull/.hg/hgrc
  $ echo "always=True" >> copy-pull/.hg/hgrc
  $ echo "[hostfingerprints]" >> copy-pull/.hg/hgrc
  $ echo "localhost =" >> copy-pull/.hg/hgrc

Test unvalidated https through proxy

  $ http_proxy=http://localhost:$HGPORT1/ hg -R copy-pull pull --insecure --traceback
  pulling from https://localhost:$HGPORT/
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  searching for changes
  no changes found

Test https with cacert and fingerprint through proxy

  $ http_proxy=http://localhost:$HGPORT1/ hg -R copy-pull pull --config web.cacerts=pub.pem
  pulling from https://localhost:$HGPORT/
  searching for changes
  no changes found
  $ http_proxy=http://localhost:$HGPORT1/ hg -R copy-pull pull https://127.0.0.1:$HGPORT/
  pulling from https://127.0.0.1:$HGPORT/
  searching for changes
  no changes found

Test https with cert problems through proxy

  $ http_proxy=http://localhost:$HGPORT1/ hg -R copy-pull pull --config web.cacerts=pub-other.pem
  pulling from https://localhost:$HGPORT/
  abort: error: *certificate verify failed* (glob)
  [255]
  $ http_proxy=http://localhost:$HGPORT1/ hg -R copy-pull pull --config web.cacerts=pub-expired.pem https://localhost:$HGPORT2/
  pulling from https://localhost:$HGPORT2/
  abort: error: *certificate verify failed* (glob)
  [255]


  $ killdaemons.py hg0.pid

#if sslcontext

Start patched hgweb that requires client certificates:

  $ cat << EOT > reqclientcert.py
  > import ssl
  > from mercurial.hgweb import server
  > class _httprequesthandlersslclientcert(server._httprequesthandlerssl):
  >     @staticmethod
  >     def preparehttpserver(httpserver, ssl_cert):
  >         sslcontext = ssl.SSLContext(ssl.PROTOCOL_TLSv1)
  >         sslcontext.verify_mode = ssl.CERT_REQUIRED
  >         sslcontext.load_cert_chain(ssl_cert)
  >         # verify clients by server certificate
  >         sslcontext.load_verify_locations(ssl_cert)
  >         httpserver.socket = sslcontext.wrap_socket(httpserver.socket,
  >                                                    server_side=True)
  > server._httprequesthandlerssl = _httprequesthandlersslclientcert
  > EOT
  $ cd test
  $ hg serve -p $HGPORT -d --pid-file=../hg0.pid --certificate=$PRIV \
  > --config extensions.reqclientcert=../reqclientcert.py
  $ cat ../hg0.pid >> $DAEMON_PIDS
  $ cd ..

without client certificate:

  $ P=`pwd` hg id https://localhost:$HGPORT/
  abort: error: *handshake failure* (glob)
  [255]

with client certificate:

  $ cat << EOT >> $HGRCPATH
  > [auth]
  > l.prefix = localhost
  > l.cert = client-cert.pem
  > l.key = client-key.pem
  > EOT

  $ P=`pwd` hg id https://localhost:$HGPORT/ \
  > --config auth.l.key=client-key-decrypted.pem
  5fed3813f7f5

  $ printf '1234\n' | env P=`pwd` hg id https://localhost:$HGPORT/ \
  > --config ui.interactive=True --config ui.nontty=True
  passphrase for client-key.pem: 5fed3813f7f5

  $ env P=`pwd` hg id https://localhost:$HGPORT/
  abort: error: * (glob)
  [255]

#endif
