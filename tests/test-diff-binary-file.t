  $ hg init a
  $ cd a
  $ cp "$TESTDIR/binfile.bin" .
  $ hg add binfile.bin
  $ hg ci -m 'add binfile.bin'

  $ echo >> binfile.bin
  $ hg ci -m 'change binfile.bin'

  $ hg revert -r 0 binfile.bin
  $ hg ci -m 'revert binfile.bin'

  $ hg diff --nodates -r 0 -r 1
  diff -r 48b371597640 -r acea2ab458c8 binfile.bin
  Binary file binfile.bin has changed

  $ hg diff --nodates -r 0 -r 2

  $ hg diff --git -r 0 -r 1
  diff --git a/binfile.bin b/binfile.bin
  index 37ba3d1c6f17137d9c5f5776fa040caf5fe73ff9..58dc31a9e2f40f74ff3b45903f7d620b8e5b7356
  GIT binary patch
  literal 594
  zc$@)J0<HatP)<h;3K|Lk000e1NJLTq000mG000mO0ssI2kdbIM00009a7bBm000XU
  z000XU0RWnu7ytkO2XskIMF-Uh9TW;VpMjwv0005-Nkl<ZD9@FWPs=e;7{<>W$NUkd
  zX$nnYLt$-$V!?uy+1V%`z&Eh=ah|duER<4|QWhju3gb^nF*8iYobxWG-qqXl=2~5M
  z*IoDB)sG^CfNuoBmqLTVU^<;@nwHP!1wrWd`{(mHo6VNXWtyh{alzqmsH*yYzpvLT
  zLdY<T=ks|woh-`&01!ej#(xbV1f|pI*=%;d-%F*E*X#ZH`4I%6SS+$EJDE&ct=8po
  ziN#{?_j|kD%Cd|oiqds`xm@;oJ-^?NG3Gdqrs?5u*zI;{nogxsx~^|Fn^Y?Gdc6<;
  zfMJ+iF1J`LMx&A2?dEwNW8ClebzPTbIh{@$hS6*`kH@1d%Lo7fA#}N1)oN7`gm$~V
  z+wDx#)OFqMcE{s!JN0-xhG8ItAjVkJwEcb`3WWlJfU2r?;Pd%dmR+q@mSri5q9_W-
  zaR2~ECX?B2w+zELozC0s*6Z~|QG^f{3I#<`?)Q7U-JZ|q5W;9Q8i_=pBuSzunx=U;
  z9C)5jBoYw9^?EHyQl(M}1OlQcCX>lXB*ODN003Z&P17_@)3Pi=i0wb04<W?v-u}7K
  zXmmQA+wDgE!qR9o8jr`%=ab_&uh(l?R=r;Tjiqon91I2-hIu?57~@*4h7h9uORK#=
  gQItJW-{SoTm)8|5##k|m00000NkvXXu0mjf3JwksH2?qr
  

  $ hg diff --git -r 0 -r 2

  $ cd ..
