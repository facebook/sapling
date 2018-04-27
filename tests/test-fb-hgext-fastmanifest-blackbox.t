Setup


Check diagnosis, debugging information
1) Setup configuration
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    hg ci -l msg
  > }

2) Set up the repo

  $ mkdir cachetesting
  $ cd cachetesting
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > fastmanifest=
  > blackbox=
  > [blackbox]
  > maxfiles=1
  > maxsize=5242880
  > track=fastmanifest
  > [fastmanifest]
  > cacheonchange=True
  > cachecutoffdays=-1
  > randomorder=False
  > EOF

  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ cat .hg/blackbox.log | grep "FM" | sed "s/.*)>//g" | egrep "(cached|skip)"
   FM: cached(rev,man) 0->a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
   FM: cached(rev,man) 1->a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
   FM: skip(rev, man) 0->a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
   FM: cached(rev,man) 2->e3738bf5439958f89499a656982023aba57b076e
   FM: skip(rev, man) 1->a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
   FM: skip(rev, man) 0->a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
