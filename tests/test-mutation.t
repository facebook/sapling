  $ enable amend rebase

We need obsmarkers for now, to allow unstable commits
  $ enable obsstore

  $ cat >> $HGRCPATH <<EOF
  > [mutation]
  > record=true
  > date=0 0
  > EOF
  $ newrepo
  $ echo "base" > base
  $ hg commit -Aqm base
  $ echo "1" > file
  $ hg commit -Aqm c1

Amend

  $ for i in 2 3 4 5 6 7 8
  > do
  >   echo $i >> file
  >   hg amend -m "c1 (amended $i)"
  > done
  $ hg debugmutation .
    c5fb4c2b7fcf4b995e8cd8f6b0cb5186d9b5b935 amend by test at 1970-01-01T00:00:00 from:
      61fdcd12ad98987cfda8da08c8e4d69f63c5fd89 amend by test at 1970-01-01T00:00:00 from:
        661239d41405ed7e61d05a207ea470ba2a81b593 amend by test at 1970-01-01T00:00:00 from:
          ac4fa5bf18651efbc4aea658be1f662cf6957b52 amend by test at 1970-01-01T00:00:00 from:
            815e611f4a75e6752f30d74f243c48cdccf4bd1e amend by test at 1970-01-01T00:00:00 from:
              c8d40e41915aa2f98b88954ce404025953dbc12a amend by test at 1970-01-01T00:00:00 from:
                4c8af5bba994ede28e843f607374031db8abd043 amend by test at 1970-01-01T00:00:00 from:
                  c5d0fa8770bdde6ef311cc640a78a2f686be28b4

Rebase

  $ echo "a" > file2
  $ hg commit -Aqm c2
  $ echo "a" > file3
  $ hg commit -Aqm c3
  $ hg rebase -q -s ".^" -d 0
  $ hg rebase -q -s ".^" -d 1 --hidden
  $ hg rebase -q -s ".^" -d 8 --hidden
  $ hg debugmutation ".^::."
    ded4fa782bd8c1051c8be550cebbc267572e15d0 rebase by test at 1970-01-01T00:00:00 from:
      33905c5919f60e31c4e4f00ad5956a06848cbe10 rebase by test at 1970-01-01T00:00:00 from:
        afdb4ea72e8cb14b34dfae49b9cc9be698468edf rebase by test at 1970-01-01T00:00:00 from:
          561937d12f41e7d2f5ade2799de1bc21b92ddc51
    8462f4f357413f9f1c76a798d6ccdfc1e4337bd7 rebase by test at 1970-01-01T00:00:00 from:
      8ae4b2d33bbb804e1e8a5d5e43164e61dfb09885 rebase by test at 1970-01-01T00:00:00 from:
        afcbdd90543ac6273d77ce2b6e967fb73373e5a4 rebase by test at 1970-01-01T00:00:00 from:
          1e2c46af1a22b8949201aee655b53f2aba83c490

