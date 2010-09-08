  $ fixheaders()
  > {
  >     sed -e 's/\(Message-Id:.*@\).*/\1/'  \
  >         -e 's/\(In-Reply-To:.*@\).*/\1/' \
  >         -e 's/\(References:.*@\).*/\1/'  \
  >         -e 's/\(User-Agent:.*\)\/.*/\1/'  \
  >         -e 's/===.*/===/'
  > }
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "patchbomb=" >> $HGRCPATH

  $ hg init t
  $ cd t
  $ echo a > a
  $ hg commit -Ama -d '1 0'
  adding a

  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -r tip
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.60@* (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  

  $ hg --config ui.interactive=1 email --confirm -n -f quux -t foo -c bar -r tip<<EOF
  > n
  > EOF
  This patch series consists of 1 patches.
  
  
  Final summary:
  
  From: quux
  To: foo
  Cc: bar
  Subject: [PATCH] a
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  are you sure you want to send (yn)? abort: patchbomb canceled
  [255]

  $ echo b > b
  $ hg commit -Amb -d '2 0'
  adding b

  $ hg email --date '1970-1-1 0:2' -n -f quux -t foo -c bar -s test -r 0:tip
  This patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  Displaying [PATCH 0 of 2] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] test
  Message-Id: <patchbomb\.120@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:02:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  Displaying [PATCH 1 of 2] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716\.121@[^>]*> (re)
  In-Reply-To: <patchbomb\.120@[^>]*> (re)
  References: <patchbomb\.120@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:02:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  Displaying [PATCH 2 of 2] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  Message-Id: <97d72e5f12c7e84f8506\.122@[^>]*> (re)
  In-Reply-To: <patchbomb\.120@[^>]*> (re)
  References: <patchbomb\.120@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:02:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

.hg/last-email.txt

  $ cat > editor << '__EOF__'
  > #!/bin/sh
  > echo "a precious introductory message" > "$1"
  > __EOF__
  $ chmod +x editor
  $ HGEDITOR="'`pwd`'"/editor hg email -n -t foo -s test -r 0:tip > /dev/null
  $ cat .hg/last-email.txt
  a precious introductory message

  $ hg email -m test.mbox -f quux -t foo -c bar -s test 0:tip \
  > --config extensions.progress= --config progress.assume-tty=1 \
  > --config progress.delay=0 --config progress.refresh=0 \
  > --config progress.width=60 2>&1 | \
  > python $TESTDIR/filtercr.py
  This patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  writing [                                             ] 0/3
  writing [                                             ] 0/3
                                                              
                                                              
  writing [==============>                              ] 1/3
  writing [==============>                              ] 1/3
                                                              
                                                              
  writing [=============================>               ] 2/3
  writing [=============================>               ] 2/3
                                                              \r (esc)
  Writing [PATCH 0 of 2] test ...
  Writing [PATCH 1 of 2] a ...
  Writing [PATCH 2 of 2] b ...
  

  $ cd ..

  $ hg clone -q t t2
  $ cd t2
  $ echo c > c
  $ hg commit -Amc -d '3 0'
  adding c

  $ cat > description <<EOF
  > a multiline
  > 
  > description
  > EOF


test bundle and description:
  $ hg email --date '1970-1-1 0:3' -n -f quux -t foo \
  >  -c bar -s test -r tip -b --desc description | fixheaders
  searching for changes
  1 changesets found
  
  Displaying test ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: test
  Message-Id: <patchbomb.180@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:03:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  a multiline
  
  description
  
  --===
  Content-Type: application/x-mercurial-bundle
  MIME-Version: 1.0
  Content-Disposition: attachment; filename="bundle.hg"
  Content-Transfer-Encoding: base64
  
  SEcxMEJaaDkxQVkmU1nvR7I3AAAN////lFYQWj1/4HwRkdC/AywIAk0E4pfoSIIIgQCgGEQOcLAA
  2tA1VPyp4mkeoG0EaaPU0GTT1GjRiNPIg9CZGBqZ6UbU9J+KFU09DNUaGgAAAAAANAGgAAAAA1U8
  oGgAADQGgAANNANAAAAAAZipFLz3XoakCEQB3PVPyHJVi1iYkAAKQAZQGpQGZESInRnCFMqLDla2
  Bx3qfRQeA2N4lnzKkAmP8kR2asievLLXXebVU8Vg4iEBqcJNJAxIapSU6SM4888ZAciRG6MYAIEE
  SlIBpFisgGkyRjX//TMtfcUAEsGu56+YnE1OlTZmzKm8BSu2rvo4rHAYYaadIFFuTy0LYgIkgLVD
  sgVa2F19D1tx9+hgbAygLgQwaIqcDdgA4BjQgIiz/AEP72++llgDKhKducqodGE4B0ETqF3JFOFC
  Q70eyNw=
  --===

utf-8 patch:
  $ python -c 'fp = open("utf", "wb"); fp.write("h\xC3\xB6mma!\n"); fp.close();'
  $ hg commit -A -d '4 0' -m 'charset=utf-8; content-transfer-encoding: base64'
  adding description
  adding utf

no mime encoding for email --test:
  $ hg email --date '1970-1-1 0:4' -f quux -t foo -c bar -r tip -n | fixheaders > mailtest

md5sum of 8-bit output:
  $ $TESTDIR/md5sum.py mailtest
  e726c29b3008e77994c7572563e57c34  mailtest

  $ rm mailtest

mime encoded mbox (base64):
  $ hg email --date '1970-1-1 0:4' -f quux -t foo -c bar -r tip -m mbox
  This patch series consists of 1 patches.
  
  
  Writing [PATCH] charset=utf-8; content-transfer-encoding: base64 ...

  $ cat mbox
  From quux Thu Jan 01 00:04:01 1970
  Content-Type: text/plain; charset="utf-8"
  MIME-Version: 1.0
  Content-Transfer-Encoding: base64
  Subject: [PATCH] charset=utf-8; content-transfer-encoding: base64
  X-Mercurial-Node: c3c9e37db9f4fe4882cda39baf42fed6bad8b15a
  Message-Id: <c3c9e37db9f4fe4882cd.240@* (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:04:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  IyBIRyBjaGFuZ2VzZXQgcGF0Y2gKIyBVc2VyIHRlc3QKIyBEYXRlIDQgMAojIE5vZGUgSUQgYzNj
  OWUzN2RiOWY0ZmU0ODgyY2RhMzliYWY0MmZlZDZiYWQ4YjE1YQojIFBhcmVudCAgZmYyYzlmYTIw
  MThiMTVmYTc0YjMzMzYzYmRhOTUyNzMyM2UyYTk5ZgpjaGFyc2V0PXV0Zi04OyBjb250ZW50LXRy
  YW5zZmVyLWVuY29kaW5nOiBiYXNlNjQKCmRpZmYgLXIgZmYyYzlmYTIwMThiIC1yIGMzYzllMzdk
  YjlmNCBkZXNjcmlwdGlvbgotLS0gL2Rldi9udWxsCVRodSBKYW4gMDEgMDA6MDA6MDAgMTk3MCAr
  MDAwMAorKysgYi9kZXNjcmlwdGlvbglUaHUgSmFuIDAxIDAwOjAwOjA0IDE5NzAgKzAwMDAKQEAg
  LTAsMCArMSwzIEBACithIG11bHRpbGluZQorCitkZXNjcmlwdGlvbgpkaWZmIC1yIGZmMmM5ZmEy
  MDE4YiAtciBjM2M5ZTM3ZGI5ZjQgdXRmCi0tLSAvZGV2L251bGwJVGh1IEphbiAwMSAwMDowMDow
  MCAxOTcwICswMDAwCisrKyBiL3V0ZglUaHUgSmFuIDAxIDAwOjAwOjA0IDE5NzAgKzAwMDAKQEAg
  LTAsMCArMSwxIEBACitow7ZtbWEhCg==
  
  
  $ rm mbox

mime encoded mbox (quoted-printable):
  $ python -c 'fp = open("qp", "wb"); fp.write("%s\nfoo\n\nbar\n" % ("x" * 1024)); fp.close();'
  $ hg commit -A -d '4 0' -m 'charset=utf-8; content-transfer-encoding: quoted-printable'
  adding qp

no mime encoding for email --test:
  $ hg email --date '1970-1-1 0:4' -f quux -t foo -c bar -r tip -n | \
  >  fixheaders > mailtest
md5sum of qp output:
  $ $TESTDIR/md5sum.py mailtest
  0402c7d033e04044e423bb04816f9dae  mailtest
  $ rm mailtest

mime encoded mbox (quoted-printable):
  $ hg email --date '1970-1-1 0:4' -f quux -t foo -c bar -r tip -m mbox
  This patch series consists of 1 patches.
  
  
  Writing [PATCH] charset=utf-8; content-transfer-encoding: quoted-printable ...
  $ cat mbox | fixheaders
  From quux Thu Jan 01 00:04:01 1970
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Subject: [PATCH] charset=utf-8; content-transfer-encoding: quoted-printable
  X-Mercurial-Node: c655633f8c87700bb38cc6a59a2753bdc5a6c376
  Message-Id: <c655633f8c87700bb38c.240@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:04:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 4 0
  # Node ID c655633f8c87700bb38cc6a59a2753bdc5a6c376
  # Parent  c3c9e37db9f4fe4882cda39baf42fed6bad8b15a
  charset=3Dutf-8; content-transfer-encoding: quoted-printable
  
  diff -r c3c9e37db9f4 -r c655633f8c87 qp
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/qp	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,4 @@
  +xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  +foo
  +
  +bar
  
  

  $ rm mbox

iso-8859-1 patch:
  $ python -c 'fp = open("isolatin", "wb"); fp.write("h\xF6mma!\n"); fp.close();'
  $ hg commit -A -d '5 0' -m 'charset=us-ascii; content-transfer-encoding: 8bit'
  adding isolatin

fake ascii mbox:
  $ hg email --date '1970-1-1 0:5' -f quux -t foo -c bar -r tip -m mbox
  This patch series consists of 1 patches.
  
  
  Writing [PATCH] charset=us-ascii; content-transfer-encoding: 8bit ...
  $ fixheaders < mbox > mboxfix

md5sum of 8-bit output:
  $ $TESTDIR/md5sum.py mboxfix
  9ea043d8fc43a71045114508baed144b  mboxfix

test diffstat for single patch:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -d -y -r 2 | \
  >  fixheaders
  This patch series consists of 1 patches.
  
  
  Final summary:
  
  From: quux
  To: foo
  Cc: bar
  Subject: [PATCH] test
   c |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  are you sure you want to send (yn)? y
  
  Displaying [PATCH] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
   c |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  

test diffstat for multiple patches:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -d -y \
  >  -r 0:1 | fixheaders
  This patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  Final summary:
  
  From: quux
  To: foo
  Cc: bar
  Subject: [PATCH 0 of 2] test
   a |  1 +
   b |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)
  Subject: [PATCH 1 of 2] a
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  Subject: [PATCH 2 of 2] b
   b |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  are you sure you want to send (yn)? y
  
  Displaying [PATCH 0 of 2] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] test
  Message-Id: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
   a |  1 +
   b |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)
  
  Displaying [PATCH 1 of 2] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.61@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  Displaying [PATCH 2 of 2] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  Message-Id: <97d72e5f12c7e84f8506.62@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
   b |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  # HG changeset patch
  # User test
  # Date 2 0
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

test inline for single patch:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -i -r 2 | \
  >  fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=t2.patch
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  --===


test inline for single patch (quoted-printable):
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -i -r 4 | \
  >  fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: c655633f8c87700bb38cc6a59a2753bdc5a6c376
  Message-Id: <c655633f8c87700bb38c.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Content-Disposition: inline; filename=t2.patch
  
  # HG changeset patch
  # User test
  # Date 4 0
  # Node ID c655633f8c87700bb38cc6a59a2753bdc5a6c376
  # Parent  c3c9e37db9f4fe4882cda39baf42fed6bad8b15a
  charset=3Dutf-8; content-transfer-encoding: quoted-printable
  
  diff -r c3c9e37db9f4 -r c655633f8c87 qp
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/qp	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,4 @@
  +xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  +foo
  +
  +bar
  
  --===

test inline for multiple patches:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -i \
  >  -r 0:1 -r 4 | fixheaders
  This patch series consists of 3 patches.
  
  
  Write the introductory message for the patch series.
  
  
  Displaying [PATCH 0 of 3] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 3] test
  Message-Id: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  Displaying [PATCH 1 of 3] a ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH 1 of 3] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.61@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=t2-1.patch
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  --===
  Displaying [PATCH 2 of 3] b ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH 2 of 3] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  Message-Id: <97d72e5f12c7e84f8506.62@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=t2-2.patch
  
  # HG changeset patch
  # User test
  # Date 2 0
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  --===
  Displaying [PATCH 3 of 3] charset=utf-8; content-transfer-encoding: quoted-printable ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH 3 of 3] charset=utf-8;
   content-transfer-encoding: quoted-printable
  X-Mercurial-Node: c655633f8c87700bb38cc6a59a2753bdc5a6c376
  Message-Id: <c655633f8c87700bb38c.63@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:03 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Content-Disposition: inline; filename=t2-3.patch
  
  # HG changeset patch
  # User test
  # Date 4 0
  # Node ID c655633f8c87700bb38cc6a59a2753bdc5a6c376
  # Parent  c3c9e37db9f4fe4882cda39baf42fed6bad8b15a
  charset=3Dutf-8; content-transfer-encoding: quoted-printable
  
  diff -r c3c9e37db9f4 -r c655633f8c87 qp
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/qp	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,4 @@
  +xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  +foo
  +
  +bar
  
  --===

test attach for single patch:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -a -r 2 | \
  >  fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  Patch subject is complete summary.
  
  
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: attachment; filename=t2.patch
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  --===

test attach for single patch (quoted-printable):
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -a -r 4 | \
  >  fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: c655633f8c87700bb38cc6a59a2753bdc5a6c376
  Message-Id: <c655633f8c87700bb38c.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  Patch subject is complete summary.
  
  
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Content-Disposition: attachment; filename=t2.patch
  
  # HG changeset patch
  # User test
  # Date 4 0
  # Node ID c655633f8c87700bb38cc6a59a2753bdc5a6c376
  # Parent  c3c9e37db9f4fe4882cda39baf42fed6bad8b15a
  charset=3Dutf-8; content-transfer-encoding: quoted-printable
  
  diff -r c3c9e37db9f4 -r c655633f8c87 qp
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/qp	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,4 @@
  +xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  +foo
  +
  +bar
  
  --===

test attach for multiple patches:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -a \
  >  -r 0:1 -r 4 | fixheaders
  This patch series consists of 3 patches.
  
  
  Write the introductory message for the patch series.
  
  
  Displaying [PATCH 0 of 3] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 3] test
  Message-Id: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  Displaying [PATCH 1 of 3] a ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH 1 of 3] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.61@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  Patch subject is complete summary.
  
  
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: attachment; filename=t2-1.patch
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  --===
  Displaying [PATCH 2 of 3] b ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH 2 of 3] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  Message-Id: <97d72e5f12c7e84f8506.62@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  Patch subject is complete summary.
  
  
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: attachment; filename=t2-2.patch
  
  # HG changeset patch
  # User test
  # Date 2 0
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  --===
  Displaying [PATCH 3 of 3] charset=utf-8; content-transfer-encoding: quoted-printable ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH 3 of 3] charset=utf-8;
   content-transfer-encoding: quoted-printable
  X-Mercurial-Node: c655633f8c87700bb38cc6a59a2753bdc5a6c376
  Message-Id: <c655633f8c87700bb38c.63@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:03 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  Patch subject is complete summary.
  
  
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Content-Disposition: attachment; filename=t2-3.patch
  
  # HG changeset patch
  # User test
  # Date 4 0
  # Node ID c655633f8c87700bb38cc6a59a2753bdc5a6c376
  # Parent  c3c9e37db9f4fe4882cda39baf42fed6bad8b15a
  charset=3Dutf-8; content-transfer-encoding: quoted-printable
  
  diff -r c3c9e37db9f4 -r c655633f8c87 qp
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/qp	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,4 @@
  +xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  +foo
  +
  +bar
  
  --===

test intro for single patch:
  $ hg email --date '1970-1-1 0:1' -n --intro -f quux -t foo -c bar -s test \
  >  -r 2 | fixheaders
  This patch series consists of 1 patches.
  
  
  Write the introductory message for the patch series.
  
  
  Displaying [PATCH 0 of 1] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 1] test
  Message-Id: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  Displaying [PATCH 1 of 1] c ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 1] c
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.61@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  

test --desc without --intro for a single patch:
  $ echo foo > intro.text
  $ hg email --date '1970-1-1 0:1' -n --desc intro.text -f quux -t foo -c bar \
  >  -s test -r 2 | fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH 0 of 1] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 1] test
  Message-Id: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  foo
  
  Displaying [PATCH 1 of 1] c ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 1] c
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.61@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  

test intro for multiple patches:
  $ hg email --date '1970-1-1 0:1' -n --intro -f quux -t foo -c bar -s test \
  >  -r 0:1 | fixheaders
  This patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  Displaying [PATCH 0 of 2] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] test
  Message-Id: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  Displaying [PATCH 1 of 2] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.61@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  Displaying [PATCH 2 of 2] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  Message-Id: <97d72e5f12c7e84f8506.62@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

test reply-to via config:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -r 2 \
  >  --config patchbomb.reply-to='baz@example.com' | fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  Reply-To: baz@example.com
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  

test reply-to via command line:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -r 2 \
  >  --reply-to baz --reply-to fred | fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  Reply-To: baz, fred
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  

tagging csets:
  $ hg tag -r0 zero zero.foo
  $ hg tag -r1 one one.patch
  $ hg tag -r2 two two.diff

test inline for single named patch:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -i -r 2 | \
  >  fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=two.diff
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  --===

test inline for multiple named/unnamed patches:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -i -r 0:1 | \
  >  fixheaders
  This patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  Displaying [PATCH 0 of 2] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] test
  Message-Id: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  Displaying [PATCH 1 of 2] a ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.61@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=t2-1.patch
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  --===
  Displaying [PATCH 2 of 2] b ...
  Content-Type: multipart/mixed; boundary="===
  MIME-Version: 1.0
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  Message-Id: <97d72e5f12c7e84f8506.62@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=one.patch
  
  # HG changeset patch
  # User test
  # Date 2 0
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  --===


test inreplyto:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar --in-reply-to baz \
  >  -r tip | fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH] Added tag two, two.diff for changeset ff2c9fa2018b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] Added tag two, two.diff for changeset ff2c9fa2018b
  X-Mercurial-Node: e317db6a6f288748d1f6cb064f3810fcba66b1b6
  Message-Id: <e317db6a6f288748d1f6.60@
  In-Reply-To: <baz>
  References: <baz>
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 0 0
  # Node ID e317db6a6f288748d1f6cb064f3810fcba66b1b6
  # Parent  eae5fcf795eee29d0e45ffc9f519a91cd79fc9ff
  Added tag two, two.diff for changeset ff2c9fa2018b
  
  diff -r eae5fcf795ee -r e317db6a6f28 .hgtags
  --- a/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -2,3 +2,5 @@
   8580ff50825a50c8f716709acdf8de0deddcd6ab zero.foo
   97d72e5f12c7e84f85064aa72e5a297142c36ed9 one
   97d72e5f12c7e84f85064aa72e5a297142c36ed9 one.patch
  +ff2c9fa2018b15fa74b33363bda9527323e2a99f two
  +ff2c9fa2018b15fa74b33363bda9527323e2a99f two.diff
  
no intro message in non-interactive mode
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar --in-reply-to baz \
  >  -r 0:1 | fixheaders
  This patch series consists of 2 patches.
  
  Subject: [PATCH 0 of 2] 
  
  Displaying [PATCH 1 of 2] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.60@
  In-Reply-To: <baz>
  References: <baz>
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  Displaying [PATCH 2 of 2] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  Message-Id: <97d72e5f12c7e84f8506.61@
  In-Reply-To: <8580ff50825a50c8f716.60@
  References: <8580ff50825a50c8f716.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  



  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar --in-reply-to baz \
  >  -s test -r 0:1 | fixheaders
  This patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  Displaying [PATCH 0 of 2] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] test
  Message-Id: <patchbomb.60@
  In-Reply-To: <baz>
  References: <baz>
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  Displaying [PATCH 1 of 2] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.61@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  Displaying [PATCH 2 of 2] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  Message-Id: <97d72e5f12c7e84f8506.62@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

test single flag for single patch:
  $ hg email --date '1970-1-1 0:1' -n --flag fooFlag -f quux -t foo -c bar -s test \
  >  -r 2 | fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH fooFlag] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH fooFlag] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  

test single flag for multiple patches:
  $ hg email --date '1970-1-1 0:1' -n --flag fooFlag -f quux -t foo -c bar -s test \
  >  -r 0:1 | fixheaders
  This patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  Displaying [PATCH 0 of 2 fooFlag] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2 fooFlag] test
  Message-Id: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  Displaying [PATCH 1 of 2 fooFlag] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2 fooFlag] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.61@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  Displaying [PATCH 2 of 2 fooFlag] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2 fooFlag] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  Message-Id: <97d72e5f12c7e84f8506.62@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

test mutiple flags for single patch:
  $ hg email --date '1970-1-1 0:1' -n --flag fooFlag --flag barFlag -f quux -t foo \
  >  -c bar -s test -r 2 | fixheaders
  This patch series consists of 1 patches.
  
  
  Displaying [PATCH fooFlag barFlag] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH fooFlag barFlag] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  

test multiple flags for multiple patches:
  $ hg email --date '1970-1-1 0:1' -n --flag fooFlag --flag barFlag -f quux -t foo \
  >  -c bar -s test -r 0:1 | fixheaders
  This patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  Displaying [PATCH 0 of 2 fooFlag barFlag] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2 fooFlag barFlag] test
  Message-Id: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  Displaying [PATCH 1 of 2 fooFlag barFlag] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2 fooFlag barFlag] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.61@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  Displaying [PATCH 2 of 2 fooFlag barFlag] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2 fooFlag barFlag] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  Message-Id: <97d72e5f12c7e84f8506.62@
  In-Reply-To: <patchbomb.60@
  References: <patchbomb.60@
  User-Agent: Mercurial-patchbomb
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

test multi-address parsing:
  $ hg email --date '1980-1-1 0:1' -m tmp.mbox -f quux -t 'spam<spam><eggs>' \
  >  -t toast -c 'foo,bar@example.com' -c '"A, B <>" <a@example.com>' -s test -r 0 \
  >  --config email.bcc='"Quux, A." <quux>'
  This patch series consists of 1 patches.
  
  
  Writing [PATCH] test ...
  $ fixheaders < tmp.mbox
  From quux Tue Jan 01 00:01:01 1980
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.315532860@
  User-Agent: Mercurial-patchbomb
  Date: Tue, 01 Jan 1980 00:01:00 +0000
  From: quux
  To: spam <spam>, eggs, toast
  Cc: foo, bar@example.com, "A, B <>" <a@example.com>
  Bcc: "Quux, A." <quux>
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  

test multi-byte domain parsing:
  $ UUML=`python -c 'import sys; sys.stdout.write("\374")'`
  $ HGENCODING=iso-8859-1
  $ export HGENCODING
  $ hg email --date '1980-1-1 0:1' -m tmp.mbox -f quux -t "bar@${UUML}nicode.com" -s test -r 0
  This patch series consists of 1 patches.
  
  Cc: 
  
  Writing [PATCH] test ...

  $ cat tmp.mbox
  From quux Tue Jan 01 00:01:01 1980
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  Message-Id: <8580ff50825a50c8f716.315532860@* (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:00 +0000
  From: quux
  To: bar@xn--nicode-2ya.com
  
  # HG changeset patch
  # User test
  # Date 1 0
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  

test outgoing:
  $ hg up 1
  0 files updated, 0 files merged, 6 files removed, 0 files unresolved

  $ hg branch test
  marked working directory as branch test

  $ echo d > d
  $ hg add d
  $ hg ci -md -d '4 0'
  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -o ../t
  comparing with ../t
  searching for changes
  From [test]: test
  This patch series consists of 8 patches.
  
  
  Write the introductory message for the patch series.
  
  Cc: 
  
  Displaying [PATCH 0 of 8] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 8] test
  Message-Id: <patchbomb.315532860@* (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:00 +0000
  From: test
  To: foo
  
  
  Displaying [PATCH 1 of 8] c ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 8] c
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  Message-Id: <ff2c9fa2018b15fa74b3.315532861@* (glob)
  In-Reply-To: <patchbomb\.315532860@[^>]*> (re)
  References: <patchbomb\.315532860@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:01 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 3 0
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  Displaying [PATCH 2 of 8] charset=utf-8; content-transfer-encoding: base64 ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 8bit
  Subject: [PATCH 2 of 8] charset=utf-8; content-transfer-encoding: base64
  X-Mercurial-Node: c3c9e37db9f4fe4882cda39baf42fed6bad8b15a
  Message-Id: <c3c9e37db9f4fe4882cd.315532862@* (glob)
  In-Reply-To: <patchbomb\.315532860@[^>]*> (re)
  References: <patchbomb\.315532860@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:02 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 4 0
  # Node ID c3c9e37db9f4fe4882cda39baf42fed6bad8b15a
  # Parent  ff2c9fa2018b15fa74b33363bda9527323e2a99f
  charset=utf-8; content-transfer-encoding: base64
  
  diff -r ff2c9fa2018b -r c3c9e37db9f4 description
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/description	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,3 @@
  +a multiline
  +
  +description
  diff -r ff2c9fa2018b -r c3c9e37db9f4 utf
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/utf	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +h\xc3\xb6mma! (esc)
  
  Displaying [PATCH 3 of 8] charset=utf-8; content-transfer-encoding: quoted-printable ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Subject: [PATCH 3 of 8] charset=utf-8;
   content-transfer-encoding: quoted-printable
  X-Mercurial-Node: c655633f8c87700bb38cc6a59a2753bdc5a6c376
  Message-Id: <c655633f8c87700bb38c.315532863@* (glob)
  In-Reply-To: <patchbomb\.315532860@[^>]*> (re)
  References: <patchbomb\.315532860@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:03 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 4 0
  # Node ID c655633f8c87700bb38cc6a59a2753bdc5a6c376
  # Parent  c3c9e37db9f4fe4882cda39baf42fed6bad8b15a
  charset=3Dutf-8; content-transfer-encoding: quoted-printable
  
  diff -r c3c9e37db9f4 -r c655633f8c87 qp
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/qp	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,4 @@
  +xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  +foo
  +
  +bar
  
  Displaying [PATCH 4 of 8] charset=us-ascii; content-transfer-encoding: 8bit ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 8bit
  Subject: [PATCH 4 of 8] charset=us-ascii; content-transfer-encoding: 8bit
  X-Mercurial-Node: 22d0f96be12f5945fd67d101af58f7bc8263c835
  Message-Id: <22d0f96be12f5945fd67.315532864@* (glob)
  In-Reply-To: <patchbomb\.315532860@[^>]*> (re)
  References: <patchbomb\.315532860@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:04 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 5 0
  # Node ID 22d0f96be12f5945fd67d101af58f7bc8263c835
  # Parent  c655633f8c87700bb38cc6a59a2753bdc5a6c376
  charset=us-ascii; content-transfer-encoding: 8bit
  
  diff -r c655633f8c87 -r 22d0f96be12f isolatin
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/isolatin	Thu Jan 01 00:00:05 1970 +0000
  @@ -0,0 +1,1 @@
  +h\xf6mma! (esc)
  
  Displaying [PATCH 5 of 8] Added tag zero, zero.foo for changeset 8580ff50825a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 5 of 8] Added tag zero, zero.foo for changeset 8580ff50825a
  X-Mercurial-Node: dd9c2b4b8a8a0934d5523c15f2c119b362360903
  Message-Id: <dd9c2b4b8a8a0934d552.315532865@* (glob)
  In-Reply-To: <patchbomb\.315532860@[^>]*> (re)
  References: <patchbomb\.315532860@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:05 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 0 0
  # Node ID dd9c2b4b8a8a0934d5523c15f2c119b362360903
  # Parent  22d0f96be12f5945fd67d101af58f7bc8263c835
  Added tag zero, zero.foo for changeset 8580ff50825a
  
  diff -r 22d0f96be12f -r dd9c2b4b8a8a .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,2 @@
  +8580ff50825a50c8f716709acdf8de0deddcd6ab zero
  +8580ff50825a50c8f716709acdf8de0deddcd6ab zero.foo
  
  Displaying [PATCH 6 of 8] Added tag one, one.patch for changeset 97d72e5f12c7 ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 6 of 8] Added tag one, one.patch for changeset 97d72e5f12c7
  X-Mercurial-Node: eae5fcf795eee29d0e45ffc9f519a91cd79fc9ff
  Message-Id: <eae5fcf795eee29d0e45.315532866@* (glob)
  In-Reply-To: <patchbomb\.315532860@[^>]*> (re)
  References: <patchbomb\.315532860@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:06 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 0 0
  # Node ID eae5fcf795eee29d0e45ffc9f519a91cd79fc9ff
  # Parent  dd9c2b4b8a8a0934d5523c15f2c119b362360903
  Added tag one, one.patch for changeset 97d72e5f12c7
  
  diff -r dd9c2b4b8a8a -r eae5fcf795ee .hgtags
  --- a/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,4 @@
   8580ff50825a50c8f716709acdf8de0deddcd6ab zero
   8580ff50825a50c8f716709acdf8de0deddcd6ab zero.foo
  +97d72e5f12c7e84f85064aa72e5a297142c36ed9 one
  +97d72e5f12c7e84f85064aa72e5a297142c36ed9 one.patch
  
  Displaying [PATCH 7 of 8] Added tag two, two.diff for changeset ff2c9fa2018b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 7 of 8] Added tag two, two.diff for changeset ff2c9fa2018b
  X-Mercurial-Node: e317db6a6f288748d1f6cb064f3810fcba66b1b6
  Message-Id: <e317db6a6f288748d1f6.315532867@* (glob)
  In-Reply-To: <patchbomb\.315532860@[^>]*> (re)
  References: <patchbomb\.315532860@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:07 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 0 0
  # Node ID e317db6a6f288748d1f6cb064f3810fcba66b1b6
  # Parent  eae5fcf795eee29d0e45ffc9f519a91cd79fc9ff
  Added tag two, two.diff for changeset ff2c9fa2018b
  
  diff -r eae5fcf795ee -r e317db6a6f28 .hgtags
  --- a/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -2,3 +2,5 @@
   8580ff50825a50c8f716709acdf8de0deddcd6ab zero.foo
   97d72e5f12c7e84f85064aa72e5a297142c36ed9 one
   97d72e5f12c7e84f85064aa72e5a297142c36ed9 one.patch
  +ff2c9fa2018b15fa74b33363bda9527323e2a99f two
  +ff2c9fa2018b15fa74b33363bda9527323e2a99f two.diff
  
  Displaying [PATCH 8 of 8] d ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 8 of 8] d
  X-Mercurial-Node: 2f9fa9b998c5fe3ac2bd9a2b14bfcbeecbc7c268
  Message-Id: <2f9fa9b998c5fe3ac2bd\.315532868[^>]*> (re)
  In-Reply-To: <patchbomb\.315532860@[^>]*> (re)
  References: <patchbomb\.315532860@[^>]*> (re)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:08 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 4 0
  # Branch test
  # Node ID 2f9fa9b998c5fe3ac2bd9a2b14bfcbeecbc7c268
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  d
  
  diff -r 97d72e5f12c7 -r 2f9fa9b998c5 d
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +d
  

dest#branch URIs:
  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -o ../t#test
  comparing with ../t
  searching for changes
  From [test]: test
  This patch series consists of 1 patches.
  
  Cc: 
  
  Displaying [PATCH] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: 2f9fa9b998c5fe3ac2bd9a2b14bfcbeecbc7c268
  Message-Id: <2f9fa9b998c5fe3ac2bd.315532860@* (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:00 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 4 0
  # Branch test
  # Node ID 2f9fa9b998c5fe3ac2bd9a2b14bfcbeecbc7c268
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  d
  
  diff -r 97d72e5f12c7 -r 2f9fa9b998c5 d
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +d
  
