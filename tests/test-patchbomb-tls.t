#require serve ssl

Set up SMTP server:

  $ CERTSDIR="$TESTDIR/sslcerts"
  $ cat "$CERTSDIR/priv.pem" "$CERTSDIR/pub.pem" >> server.pem

  $ python "$TESTDIR/dummysmtpd.py" -p $HGPORT --pid-file a.pid -d \
  > --tls smtps --certificate `pwd`/server.pem
  listening at localhost:$HGPORT
  $ cat a.pid >> $DAEMON_PIDS

Ensure hg email output is sent to stdout:

  $ unset PAGER

Set up repository:

  $ hg init t
  $ cd t
  $ cat <<EOF >> .hg/hgrc
  > [extensions]
  > patchbomb =
  > [email]
  > method = smtp
  > [smtp]
  > host = localhost
  > port = $HGPORT
  > tls = smtps
  > EOF

  $ echo a > a
  $ hg commit -Ama -d '1 0'
  adding a

Utility functions:

  $ DISABLECACERTS=
  $ try () {
  >   hg email $DISABLECACERTS -f quux -t foo -c bar -r tip "$@"
  > }

Our test cert is not signed by a trusted CA. It should fail to verify if
we are able to load CA certs:

#if defaultcacerts
  $ try
  this patch series consists of 1 patches.
  
  
  (?i)abort: .*?certificate.verify.failed.* (re)
  [255]
#endif

  $ DISABLECACERTS="--config devel.disableloaddefaultcerts=true"

Without certificates:

  $ try --debug
  this patch series consists of 1 patches.
  
  
  (using smtps)
  sending mail: smtp host localhost, port * (glob)
  (verifying remote certificate)
  warning: certificate for localhost not verified (set hostsecurity.localhost:certfingerprints=sha256:62:09:97:2f:97:60:e3:65:8f:12:5d:78:9e:35:a1:36:7a:65:4b:0e:9f:ac:db:c3:bc:6e:b6:a3:c0:16:e0:30 or web.cacerts config settings)
  sending [PATCH] a ...

With global certificates:

  $ try --debug --config web.cacerts="$CERTSDIR/pub.pem"
  this patch series consists of 1 patches.
  
  
  (using smtps)
  sending mail: smtp host localhost, port * (glob)
  (verifying remote certificate)
  sending [PATCH] a ...

With invalid certificates:

  $ try --config web.cacerts="$CERTSDIR/pub-other.pem"
  this patch series consists of 1 patches.
  
  
  (?i)abort: .*?certificate.verify.failed.* (re)
  [255]

  $ cd ..
