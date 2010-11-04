
  $ commit()
  > {
  >     msg=$1
  >     p1=$2
  >     p2=$3
  > 
  >     if [ "$p1" ]; then
  >         hg up -qC $p1
  >     fi
  > 
  >     if [ "$p2" ]; then
  >         HGMERGE=true hg merge -q $p2
  >     fi
  > 
  >     echo >> foo
  > 
  >     hg commit -qAm "$msg"
  > }
  $ hg init repo
  $ cd repo
  $ echo '[extensions]' > .hg/hgrc
  $ echo 'parentrevspec =' >> .hg/hgrc
  $ commit '0: add foo'
  $ commit '1: change foo 1'
  $ commit '2: change foo 2a'
  $ commit '3: change foo 3a'
  $ commit '4: change foo 2b' 1
  $ commit '5: merge' 3 4
  $ commit '6: change foo again'
  $ hg log --template '{rev}:{node|short} {parents}\n'
  6:755d1e0d79e9 
  5:9ce2ce29723a 3:a3e00c7dbf11 4:bb4475edb621 
  4:bb4475edb621 1:5d953a1917d1 
  3:a3e00c7dbf11 
  2:befc7d89d081 
  1:5d953a1917d1 
  0:837088b6e1d9 
  $ echo
  
  $ lookup()
  > {
  >     for rev in "$@"; do
  >         printf "$rev: "
  >         hg id -nr $rev
  >     done
  >     true
  > }
  $ tipnode=`hg id -ir tip`

should work with tag/branch/node/rev

  $ for r in tip default $tipnode 6; do
  >     lookup "$r^"
  > done
  tip^: 5
  default^: 5
  755d1e0d79e9^: 5
  6^: 5
  $ echo
  

some random lookups

  $ lookup "6^^" "6^^^" "6^^^^" "6^^^^^" "6^^^^^^" "6^1" "6^2" "6^^2" "6^1^2" "6^^3"
  6^^: 3
  6^^^: 2
  6^^^^: 1
  6^^^^^: 0
  6^^^^^^: -1
  6^1: 5
  6^2: hg: parse error at 1: syntax error
  6^^2: 4
  6^1^2: 4
  6^^3: hg: parse error at 1: syntax error
  $ lookup "6~" "6~1" "6~2" "6~3" "6~4" "6~5" "6~42" "6~1^2" "6~1^2~2"
  6~: hg: parse error at 1: syntax error
  6~1: 5
  6~2: 3
  6~3: 2
  6~4: 1
  6~5: 0
  6~42: -1
  6~1^2: 4
  6~1^2~2: 0
  $ echo
  

with a tag "6^" pointing to rev 1

  $ hg tag -l -r 1 "6^"
  $ lookup "6^" "6^1" "6~1" "6^^"
  6^: 1
  6^1: 5
  6~1: 5
  6^^: 3
  $ echo
  

with a tag "foo^bar" pointing to rev 2

  $ hg tag -l -r 2 "foo^bar"
  $ lookup "foo^bar" "foo^bar^"
  foo^bar: 2
  foo^bar^: hg: parse error at 3: syntax error
