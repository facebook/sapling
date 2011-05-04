Proper https client requires the built-in ssl from Python 2.6.

  $ "$TESTDIR/hghave" ssl || exit 80

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

  $ hg serve -p $HGPORT --certificate=$PRIV 2>&1
  abort: cannot start server at ':$HGPORT': Address already in use
  [255]
  $ cd ..

clone via pull

  $ hg clone https://localhost:$HGPORT/ copy-pull
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
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
  $ echo "changegroup = python '$TESTDIR'/printenv.py changegroup" >> .hg/hgrc
  $ hg pull
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  pulling from https://localhost:$HGPORT/
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  changegroup hook: HG_NODE=5fed3813f7f5e1824344fdc9cf8f63bb662c292d HG_SOURCE=pull HG_URL=https://localhost:$HGPORT/ 
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
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
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  pulling from https://localhost:$HGPORT/
  searching for changes
  no changes found

cacert mismatch

  $ hg -R copy-pull pull --config web.cacerts=pub.pem https://127.0.0.1:$HGPORT/
  abort: 127.0.0.1 certificate error: certificate is for localhost (use --insecure to connect insecurely)
  [255]
  $ hg -R copy-pull pull --config web.cacerts=pub.pem https://127.0.0.1:$HGPORT/ --insecure
  warning: 127.0.0.1 certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  pulling from https://127.0.0.1:$HGPORT/
  searching for changes
  no changes found
  $ hg -R copy-pull pull --config web.cacerts=pub-other.pem
  abort: error: *:SSL3_GET_SERVER_CERTIFICATE:certificate verify failed (glob)
  [255]
  $ hg -R copy-pull pull --config web.cacerts=pub-other.pem --insecure
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  pulling from https://localhost:$HGPORT/
  searching for changes
  no changes found

Test server cert which isn't valid yet

  $ hg -R test serve -p $HGPORT1 -d --pid-file=hg1.pid --certificate=server-not-yet.pem
  $ cat hg1.pid >> $DAEMON_PIDS
  $ hg -R copy-pull pull --config web.cacerts=pub-not-yet.pem https://localhost:$HGPORT1/
  abort: error: *:SSL3_GET_SERVER_CERTIFICATE:certificate verify failed (glob)
  [255]

Test server cert which no longer is valid

  $ hg -R test serve -p $HGPORT2 -d --pid-file=hg2.pid --certificate=server-expired.pem
  $ cat hg2.pid >> $DAEMON_PIDS
  $ hg -R copy-pull pull --config web.cacerts=pub-expired.pem https://localhost:$HGPORT2/
  abort: error: *:SSL3_GET_SERVER_CERTIFICATE:certificate verify failed (glob)
  [255]

Fingerprints

  $ echo "[hostfingerprints]" >> copy-pull/.hg/hgrc
  $ echo "localhost = 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca" >> copy-pull/.hg/hgrc
  $ echo "127.0.0.1 = 914f1aff87249c09b6859b88b1906d30756491ca" >> copy-pull/.hg/hgrc

- works without cacerts
  $ hg -R copy-pull id https://localhost:$HGPORT/ --config web.cacerts=
  5fed3813f7f5

- fails when cert doesn't match hostname (port is ignored)
  $ hg -R copy-pull id https://localhost:$HGPORT1/
  abort: invalid certificate for localhost with fingerprint 28:ff:71:bf:65:31:14:23:ad:62:92:b4:0e:31:99:18:fc:83:e3:9b
  [255]

- ignores that certificate doesn't match hostname
  $ hg -R copy-pull id https://127.0.0.1:$HGPORT/
  5fed3813f7f5

Prepare for connecting through proxy

  $ kill `cat hg1.pid`
  $ sleep 1

  $ ("$TESTDIR/tinyproxy.py" $HGPORT1 localhost >proxy.log 2>&1 </dev/null &
  $ echo $! > proxy.pid)
  $ cat proxy.pid >> $DAEMON_PIDS
  $ sleep 2

  $ echo "[http_proxy]" >> copy-pull/.hg/hgrc
  $ echo "always=True" >> copy-pull/.hg/hgrc
  $ echo "[hostfingerprints]" >> copy-pull/.hg/hgrc
  $ echo "localhost =" >> copy-pull/.hg/hgrc

Test unvalidated https through proxy

  $ http_proxy=http://localhost:$HGPORT1/ hg -R copy-pull pull --insecure --traceback
  warning: localhost certificate with fingerprint 91:4f:1a:ff:87:24:9c:09:b6:85:9b:88:b1:90:6d:30:75:64:91:ca not verified (check hostfingerprints or web.cacerts config setting)
  pulling from https://localhost:$HGPORT/
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
  abort: error: *:SSL3_GET_SERVER_CERTIFICATE:certificate verify failed (glob)
  [255]
  $ http_proxy=http://localhost:$HGPORT1/ hg -R copy-pull pull --config web.cacerts=pub-expired.pem https://localhost:$HGPORT2/
  abort: error: *:SSL3_GET_SERVER_CERTIFICATE:certificate verify failed (glob)
  [255]
