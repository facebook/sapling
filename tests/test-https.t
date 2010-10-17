Proper https client requires the built-in ssl from Python 2.6,
and https serve requires the full OpenSSL module.

  $ "$TESTDIR/hghave" ssl || exit 80

HTTPS serve seems to be broken on Python 2.7:

  $ [ "`python -c 'import sys; print sys.version_info[:2]'`" = '(2, 6)' ] || exit 80

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

Test server address cannot be reused

  $ hg serve -p $HGPORT --certificate=$PRIV 2>&1
  abort: cannot start server at ':$HGPORT': Address already in use
  [255]
  $ cd ..

clone via pull

  $ hg clone https://localhost:$HGPORT/ copy-pull
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

pull

  $ cd copy-pull
  $ echo '[hooks]' >> .hg/hgrc
  $ echo "changegroup = python '$TESTDIR'/printenv.py changegroup" >> .hg/hgrc
  $ hg pull
  changegroup hook: HG_NODE=5fed3813f7f5e1824344fdc9cf8f63bb662c292d HG_SOURCE=pull HG_URL=https://localhost:$HGPORT/ 
  pulling from https://localhost:$HGPORT/
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ cd ..
