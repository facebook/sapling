Note for future hackers of patchbomb: this file is a bit heavy on
wildcards in test expectations due to how many things like hostnames
tend to make it into outputs. As a result, you may need to perform the
following regular expression substitutions:
@$HOSTNAME> -> @*> (glob)
Mercurial-patchbomb/.* -> Mercurial-patchbomb/* (glob)
/mixed; boundary="===+[0-9]+==" -> /mixed; boundary="===*== (glob)"
--===+[0-9]+=+--$ -> --===*=-- (glob)
--===+[0-9]+=+$ -> --===*= (glob)

  $ cat > prune-blank-after-boundary.py <<EOF
  > import sys
  > skipblank = False
  > trim = lambda x: x.strip(' \r\n')
  > for l in sys.stdin:
  >     if trim(l).endswith('=--') or trim(l).endswith('=='):
  >         skipblank = True
  >         print l,
  >         continue
  >     if not trim(l) and skipblank:
  >         continue
  >     skipblank = False
  >     print l,
  > EOF
  $ FILTERBOUNDARY="python `pwd`/prune-blank-after-boundary.py"
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "patchbomb=" >> $HGRCPATH

  $ hg init t
  $ cd t
  $ echo a > a
  $ hg commit -Ama -d '1 0'
  adding a

  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -r tip
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <8580ff50825a50c8f716.60@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
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
  this patch series consists of 1 patches.
  
  
  Final summary:
  
  From: quux
  To: foo
  Cc: bar
  Subject: [PATCH] a
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  are you sure you want to send (yn)? n
  abort: patchbomb canceled
  [255]

  $ hg --config ui.interactive=1 --config patchbomb.confirm=true email -n -f quux -t foo -c bar -r tip<<EOF
  > n
  > EOF
  this patch series consists of 1 patches.
  
  
  Final summary:
  
  From: quux
  To: foo
  Cc: bar
  Subject: [PATCH] a
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  are you sure you want to send (yn)? n
  abort: patchbomb canceled
  [255]


Test diff.git is respected
  $ hg --config diff.git=True email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -r tip
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <8580ff50825a50c8f716.60@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +a
  


Test breaking format changes aren't
  $ hg --config diff.noprefix=True email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -r tip
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <8580ff50825a50c8f716.60@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  

  $ echo b > b
  $ hg commit -Amb -d '2 0'
  adding b

  $ hg email --date '1970-1-1 0:2' -n -f quux -t foo -c bar -s test -r 0:tip
  this patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  displaying [PATCH 0 of 2] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] test
  Message-Id: <patchbomb.120@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:02:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  displaying [PATCH 1 of 2] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 2
  Message-Id: <8580ff50825a50c8f716.121@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.121@*> (glob)
  In-Reply-To: <patchbomb.120@*> (glob)
  References: <patchbomb.120@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:02:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  displaying [PATCH 2 of 2] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 2
  Message-Id: <97d72e5f12c7e84f8506.122@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.121@*> (glob)
  In-Reply-To: <patchbomb.120@*> (glob)
  References: <patchbomb.120@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:02:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  #      Thu Jan 01 00:00:02 1970 +0000
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

.hg/last-email.txt

  $ cat > editor.sh << '__EOF__'
  > echo "a precious introductory message" > "$1"
  > __EOF__
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg email -n -t foo -s test -r 0:tip > /dev/null
  $ cat .hg/last-email.txt
  a precious introductory message

  $ hg email -m test.mbox -f quux -t foo -c bar -s test 0:tip \
  > --config extensions.progress= --config progress.assume-tty=1 \
  > --config progress.delay=0 --config progress.refresh=0 \
  > --config progress.width=60
  this patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  \r (no-eol) (esc)
  sending [                                             ] 0/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  sending [==============>                              ] 1/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  sending [=============================>               ] 2/3\r (no-eol) (esc)
                                                              \r (esc)
  sending [PATCH 0 of 2] test ...
  sending [PATCH 1 of 2] a ...
  sending [PATCH 2 of 2] b ...

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
  >  -c bar -s test -r tip -b --desc description | $FILTERBOUNDARY
  searching for changes
  1 changesets found
  
  displaying test ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: test
  Message-Id: <patchbomb.180@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:03:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  a multiline
  
  description
  
  --===*= (glob)
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
  --===*=-- (glob)

utf-8 patch:
  $ $PYTHON -c 'fp = open("utf", "wb"); fp.write("h\xC3\xB6mma!\n"); fp.close();'
  $ hg commit -A -d '4 0' -m 'utf-8 content'
  adding description
  adding utf

no mime encoding for email --test:
  $ hg email --date '1970-1-1 0:4' -f quux -t foo -c bar -r tip -n
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] utf-8 content ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 8bit
  Subject: [PATCH] utf-8 content
  X-Mercurial-Node: 909a00e13e9d78b575aeee23dddbada46d5a143f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <909a00e13e9d78b575ae.240@*> (glob)
  X-Mercurial-Series-Id: <909a00e13e9d78b575ae.240@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:04:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Node ID 909a00e13e9d78b575aeee23dddbada46d5a143f
  # Parent  ff2c9fa2018b15fa74b33363bda9527323e2a99f
  utf-8 content
  
  diff -r ff2c9fa2018b -r 909a00e13e9d description
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/description	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,3 @@
  +a multiline
  +
  +description
  diff -r ff2c9fa2018b -r 909a00e13e9d utf
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/utf	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +h\xc3\xb6mma! (esc)
  

mime encoded mbox (base64):
  $ hg email --date '1970-1-1 0:4' -f 'Q <quux>' -t foo -c bar -r tip -m mbox
  this patch series consists of 1 patches.
  
  
  sending [PATCH] utf-8 content ...

  $ cat mbox
  From quux ... ... .. ..:..:.. .... (re)
  Content-Type: text/plain; charset="utf-8"
  MIME-Version: 1.0
  Content-Transfer-Encoding: base64
  Subject: [PATCH] utf-8 content
  X-Mercurial-Node: 909a00e13e9d78b575aeee23dddbada46d5a143f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <909a00e13e9d78b575ae.240@*> (glob)
  X-Mercurial-Series-Id: <909a00e13e9d78b575ae.240@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:04:00 +0000
  From: Q <quux>
  To: foo
  Cc: bar
  
  IyBIRyBjaGFuZ2VzZXQgcGF0Y2gKIyBVc2VyIHRlc3QKIyBEYXRlIDQgMAojICAgICAgVGh1IEph
  biAwMSAwMDowMDowNCAxOTcwICswMDAwCiMgTm9kZSBJRCA5MDlhMDBlMTNlOWQ3OGI1NzVhZWVl
  MjNkZGRiYWRhNDZkNWExNDNmCiMgUGFyZW50ICBmZjJjOWZhMjAxOGIxNWZhNzRiMzMzNjNiZGE5
  NTI3MzIzZTJhOTlmCnV0Zi04IGNvbnRlbnQKCmRpZmYgLXIgZmYyYzlmYTIwMThiIC1yIDkwOWEw
  MGUxM2U5ZCBkZXNjcmlwdGlvbgotLS0gL2Rldi9udWxsCVRodSBKYW4gMDEgMDA6MDA6MDAgMTk3
  MCArMDAwMAorKysgYi9kZXNjcmlwdGlvbglUaHUgSmFuIDAxIDAwOjAwOjA0IDE5NzAgKzAwMDAK
  QEAgLTAsMCArMSwzIEBACithIG11bHRpbGluZQorCitkZXNjcmlwdGlvbgpkaWZmIC1yIGZmMmM5
  ZmEyMDE4YiAtciA5MDlhMDBlMTNlOWQgdXRmCi0tLSAvZGV2L251bGwJVGh1IEphbiAwMSAwMDow
  MDowMCAxOTcwICswMDAwCisrKyBiL3V0ZglUaHUgSmFuIDAxIDAwOjAwOjA0IDE5NzAgKzAwMDAK
  QEAgLTAsMCArMSwxIEBACitow7ZtbWEhCg==
  
  
  $ $PYTHON -c 'print open("mbox").read().split("\n\n")[1].decode("base64")'
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Node ID 909a00e13e9d78b575aeee23dddbada46d5a143f
  # Parent  ff2c9fa2018b15fa74b33363bda9527323e2a99f
  utf-8 content
  
  diff -r ff2c9fa2018b -r 909a00e13e9d description
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/description	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,3 @@
  +a multiline
  +
  +description
  diff -r ff2c9fa2018b -r 909a00e13e9d utf
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/utf	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +h\xc3\xb6mma! (esc)
  
  $ rm mbox

mime encoded mbox (quoted-printable):
  $ $PYTHON -c 'fp = open("long", "wb"); fp.write("%s\nfoo\n\nbar\n" % ("x" * 1024)); fp.close();'
  $ hg commit -A -d '4 0' -m 'long line'
  adding long

no mime encoding for email --test:
  $ hg email --date '1970-1-1 0:4' -f quux -t foo -c bar -r tip -n
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] long line ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Subject: [PATCH] long line
  X-Mercurial-Node: a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <a2ea8fc83dd8b93cfd86.240@*> (glob)
  X-Mercurial-Series-Id: <a2ea8fc83dd8b93cfd86.240@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:04:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Node ID a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  # Parent  909a00e13e9d78b575aeee23dddbada46d5a143f
  long line
  
  diff -r 909a00e13e9d -r a2ea8fc83dd8 long
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/long	Thu Jan 01 00:00:04 1970 +0000
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
  

mime encoded mbox (quoted-printable):
  $ hg email --date '1970-1-1 0:4' -f quux -t foo -c bar -r tip -m mbox
  this patch series consists of 1 patches.
  
  
  sending [PATCH] long line ...
  $ cat mbox
  From quux ... ... .. ..:..:.. .... (re)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Subject: [PATCH] long line
  X-Mercurial-Node: a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <a2ea8fc83dd8b93cfd86.240@*> (glob)
  X-Mercurial-Series-Id: <a2ea8fc83dd8b93cfd86.240@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:04:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Node ID a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  # Parent  909a00e13e9d78b575aeee23dddbada46d5a143f
  long line
  
  diff -r 909a00e13e9d -r a2ea8fc83dd8 long
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/long	Thu Jan 01 00:00:04 1970 +0000
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
  $ $PYTHON -c 'fp = open("isolatin", "wb"); fp.write("h\xF6mma!\n"); fp.close();'
  $ hg commit -A -d '5 0' -m 'isolatin 8-bit encoding'
  adding isolatin

fake ascii mbox:
  $ hg email --date '1970-1-1 0:5' -f quux -t foo -c bar -r tip -m mbox
  this patch series consists of 1 patches.
  
  
  sending [PATCH] isolatin 8-bit encoding ...
  $ cat mbox
  From quux ... ... .. ..:..:.. .... (re)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 8bit
  Subject: [PATCH] isolatin 8-bit encoding
  X-Mercurial-Node: 240fb913fc1b7ff15ddb9f33e73d82bf5277c720
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <240fb913fc1b7ff15ddb.300@*> (glob)
  X-Mercurial-Series-Id: <240fb913fc1b7ff15ddb.300@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:05:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 5 0
  #      Thu Jan 01 00:00:05 1970 +0000
  # Node ID 240fb913fc1b7ff15ddb9f33e73d82bf5277c720
  # Parent  a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  isolatin 8-bit encoding
  
  diff -r a2ea8fc83dd8 -r 240fb913fc1b isolatin
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/isolatin	Thu Jan 01 00:00:05 1970 +0000
  @@ -0,0 +1,1 @@
  +h\xf6mma! (esc)
  
  

test diffstat for single patch:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -d -y -r 2
  this patch series consists of 1 patches.
  
  
  Final summary:
  
  From: quux
  To: foo
  Cc: bar
  Subject: [PATCH] test
   c |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  are you sure you want to send (yn)? y
  
  displaying [PATCH] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
   c |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
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
  >  -r 0:1
  this patch series consists of 2 patches.
  
  
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
  
  displaying [PATCH 0 of 2] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] test
  Message-Id: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
   a |  1 +
   b |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)
  
  displaying [PATCH 1 of 2] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 2
  Message-Id: <8580ff50825a50c8f716.61@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  displaying [PATCH 2 of 2] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 2
  Message-Id: <97d72e5f12c7e84f8506.62@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
   b |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  # HG changeset patch
  # User test
  # Date 2 0
  #      Thu Jan 01 00:00:02 1970 +0000
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

test inline for single patch:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -i -r 2 | $FILTERBOUNDARY
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=t2.patch
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  --===*=-- (glob)


test inline for single patch (quoted-printable):
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -i -r 4 | $FILTERBOUNDARY
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <a2ea8fc83dd8b93cfd86.60@*> (glob)
  X-Mercurial-Series-Id: <a2ea8fc83dd8b93cfd86.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Content-Disposition: inline; filename=t2.patch
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Node ID a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  # Parent  909a00e13e9d78b575aeee23dddbada46d5a143f
  long line
  
  diff -r 909a00e13e9d -r a2ea8fc83dd8 long
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/long	Thu Jan 01 00:00:04 1970 +0000
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
  
  --===*=-- (glob)

test inline for multiple patches:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -i \
  >  -r 0:1 -r 4 | $FILTERBOUNDARY
  this patch series consists of 3 patches.
  
  
  Write the introductory message for the patch series.
  
  
  displaying [PATCH 0 of 3] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 3] test
  Message-Id: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  displaying [PATCH 1 of 3] a ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH 1 of 3] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 3
  Message-Id: <8580ff50825a50c8f716.61@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=t2-1.patch
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  --===*=-- (glob)
  displaying [PATCH 2 of 3] b ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH 2 of 3] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 3
  Message-Id: <97d72e5f12c7e84f8506.62@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=t2-2.patch
  
  # HG changeset patch
  # User test
  # Date 2 0
  #      Thu Jan 01 00:00:02 1970 +0000
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  --===*=-- (glob)
  displaying [PATCH 3 of 3] long line ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH 3 of 3] long line
  X-Mercurial-Node: a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  X-Mercurial-Series-Index: 3
  X-Mercurial-Series-Total: 3
  Message-Id: <a2ea8fc83dd8b93cfd86.63@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:03 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Content-Disposition: inline; filename=t2-3.patch
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Node ID a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  # Parent  909a00e13e9d78b575aeee23dddbada46d5a143f
  long line
  
  diff -r 909a00e13e9d -r a2ea8fc83dd8 long
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/long	Thu Jan 01 00:00:04 1970 +0000
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
  
  --===*=-- (glob)

test attach for single patch:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -a -r 2 | $FILTERBOUNDARY
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  Patch subject is complete summary.
  
  
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: attachment; filename=t2.patch
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  --===*=-- (glob)

test attach for single patch (quoted-printable):
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -a -r 4 | $FILTERBOUNDARY
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <a2ea8fc83dd8b93cfd86.60@*> (glob)
  X-Mercurial-Series-Id: <a2ea8fc83dd8b93cfd86.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  Patch subject is complete summary.
  
  
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Content-Disposition: attachment; filename=t2.patch
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Node ID a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  # Parent  909a00e13e9d78b575aeee23dddbada46d5a143f
  long line
  
  diff -r 909a00e13e9d -r a2ea8fc83dd8 long
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/long	Thu Jan 01 00:00:04 1970 +0000
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
  
  --===*=-- (glob)

test attach and body for single patch:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -a --body -r 2 | $FILTERBOUNDARY
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: attachment; filename=t2.patch
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  --===*=-- (glob)

test attach for multiple patches:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -a \
  >  -r 0:1 -r 4 | $FILTERBOUNDARY
  this patch series consists of 3 patches.
  
  
  Write the introductory message for the patch series.
  
  
  displaying [PATCH 0 of 3] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 3] test
  Message-Id: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  displaying [PATCH 1 of 3] a ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH 1 of 3] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 3
  Message-Id: <8580ff50825a50c8f716.61@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  Patch subject is complete summary.
  
  
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: attachment; filename=t2-1.patch
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  --===*=-- (glob)
  displaying [PATCH 2 of 3] b ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH 2 of 3] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 3
  Message-Id: <97d72e5f12c7e84f8506.62@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  Patch subject is complete summary.
  
  
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: attachment; filename=t2-2.patch
  
  # HG changeset patch
  # User test
  # Date 2 0
  #      Thu Jan 01 00:00:02 1970 +0000
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  --===*=-- (glob)
  displaying [PATCH 3 of 3] long line ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH 3 of 3] long line
  X-Mercurial-Node: a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  X-Mercurial-Series-Index: 3
  X-Mercurial-Series-Total: 3
  Message-Id: <a2ea8fc83dd8b93cfd86.63@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:03 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  
  Patch subject is complete summary.
  
  
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Content-Disposition: attachment; filename=t2-3.patch
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Node ID a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  # Parent  909a00e13e9d78b575aeee23dddbada46d5a143f
  long line
  
  diff -r 909a00e13e9d -r a2ea8fc83dd8 long
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/long	Thu Jan 01 00:00:04 1970 +0000
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
  
  --===*=-- (glob)

test intro for single patch:
  $ hg email --date '1970-1-1 0:1' -n --intro -f quux -t foo -c bar -s test \
  >  -r 2
  this patch series consists of 1 patches.
  
  
  Write the introductory message for the patch series.
  
  
  displaying [PATCH 0 of 1] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 1] test
  Message-Id: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  displaying [PATCH 1 of 1] c ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 1] c
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.61@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
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
  >  -s test -r 2
  this patch series consists of 1 patches.
  
  
  displaying [PATCH 0 of 1] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 1] test
  Message-Id: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  foo
  
  displaying [PATCH 1 of 1] c ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 1] c
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.61@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
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
  >  -r 0:1
  this patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  displaying [PATCH 0 of 2] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] test
  Message-Id: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  displaying [PATCH 1 of 2] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 2
  Message-Id: <8580ff50825a50c8f716.61@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  displaying [PATCH 2 of 2] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 2
  Message-Id: <97d72e5f12c7e84f8506.62@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  #      Thu Jan 01 00:00:02 1970 +0000
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
  >  --config patchbomb.reply-to='baz@example.com'
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  Reply-To: baz@example.com
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
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
  >  --reply-to baz --reply-to fred
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  Reply-To: baz, fred
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
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
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -i \
  >   -r 2 | $FILTERBOUNDARY
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] test ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=two.diff
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  --===*=-- (glob)

test inline for multiple named/unnamed patches:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar -s test -i \
  >    -r 0:1 | $FILTERBOUNDARY
  this patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  displaying [PATCH 0 of 2] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] test
  Message-Id: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  displaying [PATCH 1 of 2] a ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 2
  Message-Id: <8580ff50825a50c8f716.61@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=t2-1.patch
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  --===*=-- (glob)
  displaying [PATCH 2 of 2] b ...
  Content-Type: multipart/mixed; boundary="===*==" (glob)
  MIME-Version: 1.0
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 2
  Message-Id: <97d72e5f12c7e84f8506.62@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  --===*= (glob)
  Content-Type: text/x-patch; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Content-Disposition: inline; filename=one.patch
  
  # HG changeset patch
  # User test
  # Date 2 0
  #      Thu Jan 01 00:00:02 1970 +0000
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  --===*=-- (glob)


test inreplyto:
  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar --in-reply-to baz \
  >  -r tip
  this patch series consists of 1 patches.
  
  
  displaying [PATCH] Added tag two, two.diff for changeset ff2c9fa2018b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] Added tag two, two.diff for changeset ff2c9fa2018b
  X-Mercurial-Node: 7aead2484924c445ad8ce2613df91f52f9e502ed
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <7aead2484924c445ad8c.60@*> (glob)
  X-Mercurial-Series-Id: <7aead2484924c445ad8c.60@*> (glob)
  In-Reply-To: <baz>
  References: <baz>
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 7aead2484924c445ad8ce2613df91f52f9e502ed
  # Parent  045ca29b1ea20e4940411e695e20e521f2f0f98e
  Added tag two, two.diff for changeset ff2c9fa2018b
  
  diff -r 045ca29b1ea2 -r 7aead2484924 .hgtags
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
  >  -r 0:1
  this patch series consists of 2 patches.
  
  (optional) Subject: [PATCH 0 of 2] 
  
  displaying [PATCH 1 of 2] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 2
  Message-Id: <8580ff50825a50c8f716.60@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.60@*> (glob)
  In-Reply-To: <baz>
  References: <baz>
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  displaying [PATCH 2 of 2] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 2
  Message-Id: <97d72e5f12c7e84f8506.61@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.60@*> (glob)
  In-Reply-To: <baz>
  References: <baz>
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  #      Thu Jan 01 00:00:02 1970 +0000
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  



  $ hg email --date '1970-1-1 0:1' -n -f quux -t foo -c bar --in-reply-to baz \
  >  -s test -r 0:1
  this patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  displaying [PATCH 0 of 2] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] test
  Message-Id: <patchbomb.60@*> (glob)
  In-Reply-To: <baz>
  References: <baz>
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  displaying [PATCH 1 of 2] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 2
  Message-Id: <8580ff50825a50c8f716.61@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  displaying [PATCH 2 of 2] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 2
  Message-Id: <97d72e5f12c7e84f8506.62@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  #      Thu Jan 01 00:00:02 1970 +0000
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

test single flag for single patch (and no warning when not mailing dirty rev):
  $ hg up -qr1
  $ echo dirt > a
  $ hg email --date '1970-1-1 0:1' -n --flag fooFlag -f quux -t foo -c bar -s test \
  >  -r 2 | $FILTERBOUNDARY
  this patch series consists of 1 patches.
  
  
  displaying [PATCH fooFlag] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH fooFlag] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  

test single flag for multiple patches (and warning when mailing dirty rev):
  $ hg email --date '1970-1-1 0:1' -n --flag fooFlag -f quux -t foo -c bar -s test \
  >  -r 0:1
  warning: working directory has uncommitted changes
  this patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  displaying [PATCH 0 of 2 fooFlag] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2 fooFlag] test
  Message-Id: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  displaying [PATCH 1 of 2 fooFlag] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2 fooFlag] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 2
  Message-Id: <8580ff50825a50c8f716.61@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  displaying [PATCH 2 of 2 fooFlag] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2 fooFlag] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 2
  Message-Id: <97d72e5f12c7e84f8506.62@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  #      Thu Jan 01 00:00:02 1970 +0000
  # Node ID 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  # Parent  8580ff50825a50c8f716709acdf8de0deddcd6ab
  b
  
  diff -r 8580ff50825a -r 97d72e5f12c7 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  $ hg revert --no-b a
  $ hg up -q

test multiple flags for single patch:
  $ hg email --date '1970-1-1 0:1' -n --flag fooFlag --flag barFlag -f quux -t foo \
  >  -c bar -s test -r 2
  this patch series consists of 1 patches.
  
  
  displaying [PATCH fooFlag barFlag] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH fooFlag barFlag] test
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
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
  >  -c bar -s test -r 0:1
  this patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  
  displaying [PATCH 0 of 2 fooFlag barFlag] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2 fooFlag barFlag] test
  Message-Id: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:00 +0000
  From: quux
  To: foo
  Cc: bar
  
  
  displaying [PATCH 1 of 2 fooFlag barFlag] a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2 fooFlag barFlag] a
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 2
  Message-Id: <8580ff50825a50c8f716.61@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:01 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  displaying [PATCH 2 of 2 fooFlag barFlag] b ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2 fooFlag barFlag] b
  X-Mercurial-Node: 97d72e5f12c7e84f85064aa72e5a297142c36ed9
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 2
  Message-Id: <97d72e5f12c7e84f8506.62@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.61@*> (glob)
  In-Reply-To: <patchbomb.60@*> (glob)
  References: <patchbomb.60@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1970 00:01:02 +0000
  From: quux
  To: foo
  Cc: bar
  
  # HG changeset patch
  # User test
  # Date 2 0
  #      Thu Jan 01 00:00:02 1970 +0000
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
  this patch series consists of 1 patches.
  
  
  sending [PATCH] test ...
  $ cat < tmp.mbox
  From quux ... ... .. ..:..:.. .... (re)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <8580ff50825a50c8f716.315532860@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.315532860@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:00 +0000
  From: quux
  To: spam <spam>, eggs, toast
  Cc: foo, bar@example.com, "A, B <>" <a@example.com>
  Bcc: "Quux, A." <quux>
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
  # Node ID 8580ff50825a50c8f716709acdf8de0deddcd6ab
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  

test multi-byte domain parsing:
  $ UUML=`$PYTHON -c 'import sys; sys.stdout.write("\374")'`
  $ HGENCODING=iso-8859-1
  $ export HGENCODING
  $ hg email --date '1980-1-1 0:1' -m tmp.mbox -f quux -t "bar@${UUML}nicode.com" -s test -r 0
  this patch series consists of 1 patches.
  
  Cc: 
  
  sending [PATCH] test ...

  $ cat tmp.mbox
  From quux ... ... .. ..:..:.. .... (re)
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: 8580ff50825a50c8f716709acdf8de0deddcd6ab
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <8580ff50825a50c8f716.315532860@*> (glob)
  X-Mercurial-Series-Id: <8580ff50825a50c8f716.315532860@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:00 +0000
  From: quux
  To: bar@xn--nicode-2ya.com
  
  # HG changeset patch
  # User test
  # Date 1 0
  #      Thu Jan 01 00:00:01 1970 +0000
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
  (branches are permanent and global, did you want a bookmark?)

  $ echo d > d
  $ hg add d
  $ hg ci -md -d '4 0'
  $ echo d >> d
  $ hg ci -mdd -d '5 0'
  $ hg log -G --template "{rev}:{node|short} {desc|firstline}\n"
  @  10:3b6f1ec9dde9 dd
  |
  o  9:2f9fa9b998c5 d
  |
  | o  8:7aead2484924 Added tag two, two.diff for changeset ff2c9fa2018b
  | |
  | o  7:045ca29b1ea2 Added tag one, one.patch for changeset 97d72e5f12c7
  | |
  | o  6:5d5ef15dfe5e Added tag zero, zero.foo for changeset 8580ff50825a
  | |
  | o  5:240fb913fc1b isolatin 8-bit encoding
  | |
  | o  4:a2ea8fc83dd8 long line
  | |
  | o  3:909a00e13e9d utf-8 content
  | |
  | o  2:ff2c9fa2018b c
  |/
  o  1:97d72e5f12c7 b
  |
  o  0:8580ff50825a a
  
  $ hg phase --force --secret -r 10
  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -o ../t -r 'rev(10) or rev(6)'
  comparing with ../t
  From [test]: test
  this patch series consists of 6 patches.
  
  
  Write the introductory message for the patch series.
  
  Cc: 
  
  displaying [PATCH 0 of 6] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 6] test
  Message-Id: <patchbomb.315532860@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:00 +0000
  From: test
  To: foo
  
  
  displaying [PATCH 1 of 6] c ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 6] c
  X-Mercurial-Node: ff2c9fa2018b15fa74b33363bda9527323e2a99f
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 6
  Message-Id: <ff2c9fa2018b15fa74b3.315532861@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.315532861@*> (glob)
  In-Reply-To: <patchbomb.315532860@*> (glob)
  References: <patchbomb.315532860@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:01 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 3 0
  #      Thu Jan 01 00:00:03 1970 +0000
  # Node ID ff2c9fa2018b15fa74b33363bda9527323e2a99f
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  c
  
  diff -r 97d72e5f12c7 -r ff2c9fa2018b c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  displaying [PATCH 2 of 6] utf-8 content ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 8bit
  Subject: [PATCH 2 of 6] utf-8 content
  X-Mercurial-Node: 909a00e13e9d78b575aeee23dddbada46d5a143f
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 6
  Message-Id: <909a00e13e9d78b575ae.315532862@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.315532861@*> (glob)
  In-Reply-To: <patchbomb.315532860@*> (glob)
  References: <patchbomb.315532860@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:02 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Node ID 909a00e13e9d78b575aeee23dddbada46d5a143f
  # Parent  ff2c9fa2018b15fa74b33363bda9527323e2a99f
  utf-8 content
  
  diff -r ff2c9fa2018b -r 909a00e13e9d description
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/description	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,3 @@
  +a multiline
  +
  +description
  diff -r ff2c9fa2018b -r 909a00e13e9d utf
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/utf	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +h\xc3\xb6mma! (esc)
  
  displaying [PATCH 3 of 6] long line ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: quoted-printable
  Subject: [PATCH 3 of 6] long line
  X-Mercurial-Node: a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  X-Mercurial-Series-Index: 3
  X-Mercurial-Series-Total: 6
  Message-Id: <a2ea8fc83dd8b93cfd86.315532863@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.315532861@*> (glob)
  In-Reply-To: <patchbomb.315532860@*> (glob)
  References: <patchbomb.315532860@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:03 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Node ID a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  # Parent  909a00e13e9d78b575aeee23dddbada46d5a143f
  long line
  
  diff -r 909a00e13e9d -r a2ea8fc83dd8 long
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/long	Thu Jan 01 00:00:04 1970 +0000
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
  
  displaying [PATCH 4 of 6] isolatin 8-bit encoding ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 8bit
  Subject: [PATCH 4 of 6] isolatin 8-bit encoding
  X-Mercurial-Node: 240fb913fc1b7ff15ddb9f33e73d82bf5277c720
  X-Mercurial-Series-Index: 4
  X-Mercurial-Series-Total: 6
  Message-Id: <240fb913fc1b7ff15ddb.315532864@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.315532861@*> (glob)
  In-Reply-To: <patchbomb.315532860@*> (glob)
  References: <patchbomb.315532860@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:04 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 5 0
  #      Thu Jan 01 00:00:05 1970 +0000
  # Node ID 240fb913fc1b7ff15ddb9f33e73d82bf5277c720
  # Parent  a2ea8fc83dd8b93cfd86ac97b28287204ab806e1
  isolatin 8-bit encoding
  
  diff -r a2ea8fc83dd8 -r 240fb913fc1b isolatin
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/isolatin	Thu Jan 01 00:00:05 1970 +0000
  @@ -0,0 +1,1 @@
  +h\xf6mma! (esc)
  
  displaying [PATCH 5 of 6] Added tag zero, zero.foo for changeset 8580ff50825a ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 5 of 6] Added tag zero, zero.foo for changeset 8580ff50825a
  X-Mercurial-Node: 5d5ef15dfe5e7bd3a4ee154b5fff76c7945ec433
  X-Mercurial-Series-Index: 5
  X-Mercurial-Series-Total: 6
  Message-Id: <5d5ef15dfe5e7bd3a4ee.315532865@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.315532861@*> (glob)
  In-Reply-To: <patchbomb.315532860@*> (glob)
  References: <patchbomb.315532860@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:05 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 5d5ef15dfe5e7bd3a4ee154b5fff76c7945ec433
  # Parent  240fb913fc1b7ff15ddb9f33e73d82bf5277c720
  Added tag zero, zero.foo for changeset 8580ff50825a
  
  diff -r 240fb913fc1b -r 5d5ef15dfe5e .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,2 @@
  +8580ff50825a50c8f716709acdf8de0deddcd6ab zero
  +8580ff50825a50c8f716709acdf8de0deddcd6ab zero.foo
  
  displaying [PATCH 6 of 6] d ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 6 of 6] d
  X-Mercurial-Node: 2f9fa9b998c5fe3ac2bd9a2b14bfcbeecbc7c268
  X-Mercurial-Series-Index: 6
  X-Mercurial-Series-Total: 6
  Message-Id: <2f9fa9b998c5fe3ac2bd.315532866@*> (glob)
  X-Mercurial-Series-Id: <ff2c9fa2018b15fa74b3.315532861@*> (glob)
  In-Reply-To: <patchbomb.315532860@*> (glob)
  References: <patchbomb.315532860@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:06 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
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
  From [test]: test
  this patch series consists of 1 patches.
  
  Cc: 
  
  displaying [PATCH] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: 2f9fa9b998c5fe3ac2bd9a2b14bfcbeecbc7c268
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <2f9fa9b998c5fe3ac2bd.315532860@*> (glob)
  X-Mercurial-Series-Id: <2f9fa9b998c5fe3ac2bd.315532860@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:00 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 4 0
  #      Thu Jan 01 00:00:04 1970 +0000
  # Branch test
  # Node ID 2f9fa9b998c5fe3ac2bd9a2b14bfcbeecbc7c268
  # Parent  97d72e5f12c7e84f85064aa72e5a297142c36ed9
  d
  
  diff -r 97d72e5f12c7 -r 2f9fa9b998c5 d
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +d
  

Test introduction configuration
=================================

  $ echo '[patchbomb]' >> $HGRCPATH

"auto" setting
----------------

  $ echo 'intro=auto' >> $HGRCPATH

single rev

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '10' | grep "Write the introductory message for the patch series."
  [1]

single rev + flag

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '10' --intro | grep "Write the introductory message for the patch series."
  Write the introductory message for the patch series.


Multi rev

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '9::' | grep "Write the introductory message for the patch series."
  Write the introductory message for the patch series.

"never" setting
-----------------

  $ echo 'intro=never' >> $HGRCPATH

single rev

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '10' | grep "Write the introductory message for the patch series."
  [1]

single rev + flag

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '10' --intro | grep "Write the introductory message for the patch series."
  Write the introductory message for the patch series.


Multi rev

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '9::' | grep "Write the introductory message for the patch series."
  [1]

Multi rev + flag

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '9::' --intro | grep "Write the introductory message for the patch series."
  Write the introductory message for the patch series.

"always" setting
-----------------

  $ echo 'intro=always' >> $HGRCPATH

single rev

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '10' | grep "Write the introductory message for the patch series."
  Write the introductory message for the patch series.

single rev + flag

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '10' --intro | grep "Write the introductory message for the patch series."
  Write the introductory message for the patch series.


Multi rev

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '9::' | grep "Write the introductory message for the patch series."
  Write the introductory message for the patch series.

Multi rev + flag

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '9::' --intro | grep "Write the introductory message for the patch series."
  Write the introductory message for the patch series.

bad value setting
-----------------

  $ echo 'intro=mpmwearaclownnose' >> $HGRCPATH

single rev

  $ hg email --date '1980-1-1 0:1' -n -t foo -s test -r '10'
  From [test]: test
  this patch series consists of 1 patches.
  
  warning: invalid patchbomb.intro value "mpmwearaclownnose"
  (should be one of always, never, auto)
  Cc: 
  
  displaying [PATCH] test ...
  Content-Type: text/plain; charset="us-ascii"
  MIME-Version: 1.0
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] test
  X-Mercurial-Node: 3b6f1ec9dde933a40a115a7990f8b320477231af
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <3b6f1ec9dde933a40a11*> (glob)
  X-Mercurial-Series-Id: <3b6f1ec9dde933a40a11.*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Tue, 01 Jan 1980 00:01:00 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 5 0
  #      Thu Jan 01 00:00:05 1970 +0000
  # Branch test
  # Node ID 3b6f1ec9dde933a40a115a7990f8b320477231af
  # Parent  2f9fa9b998c5fe3ac2bd9a2b14bfcbeecbc7c268
  dd
  
  diff -r 2f9fa9b998c5 -r 3b6f1ec9dde9 d
  --- a/d	Thu Jan 01 00:00:04 1970 +0000
  +++ b/d	Thu Jan 01 00:00:05 1970 +0000
  @@ -1,1 +1,2 @@
   d
  +d
  
