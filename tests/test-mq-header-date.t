
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > mq =
  > [diff]
  > nodates = true
  > EOF
  $ catpatch() {
  >     cat .hg/patches/$1.patch | sed -e "s/^diff \-r [0-9a-f]* /diff -r ... /" \
  >                                    -e "s/^\(# Parent \).*/\1/"
  > }
  $ catlog() {
  >     catpatch $1
  >     hg log --template "{rev}: {node|short} {desc} - {author}\n"
  > }
  $ catlogd() {
  >     catpatch $1
  >     hg log --template "{rev}: {node|short} {desc} - {author} - {date}\n"
  > }
  $ drop() {
  >     hg qpop
  >     hg qdel $1.patch
  > }
  $ runtest() {
  >     echo ==== init
  >     hg init a
  >     cd a
  >     hg qinit
  > 
  > 
  >     echo ==== qnew -d
  >     hg qnew -d '3 0' 1.patch
  >     catlogd 1
  > 
  >     echo ==== qref
  >     echo "1" >1
  >     hg add
  >     hg qref
  >     catlogd 1
  > 
  >     echo ==== qref -d
  >     hg qref -d '4 0'
  >     catlogd 1
  > 
  > 
  >     echo ==== qnew
  >     hg qnew 2.patch
  >     echo "2" >2
  >     hg add
  >     hg qref
  >     catlog 2
  > 
  >     echo ==== qref -d
  >     hg qref -d '5 0'
  >     catlog 2
  > 
  >     drop 2
  > 
  > 
  >     echo ==== qnew -d -m
  >     hg qnew -d '6 0' -m "Three" 3.patch
  >     catlogd 3
  > 
  >     echo ==== qref
  >     echo "3" >3
  >     hg add
  >     hg qref
  >     catlogd 3
  > 
  >     echo ==== qref -m
  >     hg qref -m "Drei"
  >     catlogd 3
  > 
  >     echo ==== qref -d
  >     hg qref -d '7 0'
  >     catlogd 3
  > 
  >     echo ==== qref -d -m
  >     hg qref -d '8 0' -m "Three (again)"
  >     catlogd 3
  > 
  > 
  >     echo ==== qnew -m
  >     hg qnew -m "Four" 4.patch
  >     echo "4" >4
  >     hg add
  >     hg qref
  >     catlog 4
  > 
  >     echo ==== qref -d
  >     hg qref -d '9 0'
  >     catlog 4
  > 
  >     drop 4
  > 
  > 
  >     echo ==== qnew with HG header
  >     hg qnew --config 'mq.plain=true' 5.patch
  >     hg qpop
  >     echo "# HG changeset patch" >>.hg/patches/5.patch
  >     echo "# Date 10 0" >>.hg/patches/5.patch
  >     hg qpush 2>&1 | grep 'Now at'
  >     catlogd 5
  > 
  >     echo ==== hg qref
  >     echo "5" >5
  >     hg add
  >     hg qref
  >     catlogd 5
  > 
  >     echo ==== hg qref -d
  >     hg qref -d '11 0'
  >     catlogd 5
  > 
  > 
  >     echo ==== qnew with plain header
  >     hg qnew --config 'mq.plain=true' -d '12 0' 6.patch
  >     hg qpop
  >     hg qpush 2>&1 | grep 'now at'
  >     catlog 6
  > 
  >     echo ==== hg qref
  >     echo "6" >6
  >     hg add
  >     hg qref
  >     catlogd 6
  > 
  >     echo ==== hg qref -d
  >     hg qref -d '13 0'
  >     catlogd 6
  > 
  >     drop 6
  > 
  > 
  >     echo ==== qnew -u
  >     hg qnew -u jane 6.patch
  >     echo "6" >6
  >     hg add
  >     hg qref
  >     catlog 6
  > 
  >     echo ==== qref -d
  >     hg qref -d '12 0'
  >     catlog 6
  > 
  >     drop 6
  > 
  > 
  >     echo ==== qnew -d
  >     hg qnew -d '13 0' 7.patch
  >     echo "7" >7
  >     hg add
  >     hg qref
  >     catlog 7
  > 
  >     echo ==== qref -u
  >     hg qref -u john
  >     catlogd 7
  > 
  > 
  >     echo ==== qnew
  >     hg qnew 8.patch
  >     echo "8" >8
  >     hg add
  >     hg qref
  >     catlog 8
  > 
  >     echo ==== qref -u -d
  >     hg qref -u john -d '14 0'
  >     catlog 8
  > 
  >     drop 8
  > 
  > 
  >     echo ==== qnew -m
  >     hg qnew -m "Nine" 9.patch
  >     echo "9" >9
  >     hg add
  >     hg qref
  >     catlog 9
  > 
  >     echo ==== qref -u -d
  >     hg qref -u john -d '15 0'
  >     catlog 9
  > 
  >     drop 9
  > 
  > 
  >     echo ==== "qpop -a / qpush -a"
  >     hg qpop -a
  >     hg qpush -a
  >     hg log --template "{rev}: {node|short} {desc} - {author} - {date}\n"
  > }

======= plain headers

  $ echo "[mq]" >> $HGRCPATH
  $ echo "plain=true" >> $HGRCPATH
  $ mkdir sandbox
  $ (cd sandbox ; runtest)
  ==== init
  ==== qnew -d
  Date: 3 0
  
  0: 758bd2596a39 [mq]: 1.patch - test - 3.00
  ==== qref
  adding 1
  Date: 3 0
  
  diff -r ... 1
  --- /dev/null
  +++ b/1
  @@ -0,0 +1,1 @@
  +1
  0: 8c640e9949a8 [mq]: 1.patch - test - 3.00
  ==== qref -d
  Date: 4 0
  
  diff -r ... 1
  --- /dev/null
  +++ b/1
  @@ -0,0 +1,1 @@
  +1
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qnew
  adding 2
  diff -r ... 2
  --- /dev/null
  +++ b/2
  @@ -0,0 +1,1 @@
  +2
  1: fc7e8a2f6499 [mq]: 2.patch - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -d
  Date: 5 0
  
  diff -r ... 2
  --- /dev/null
  +++ b/2
  @@ -0,0 +1,1 @@
  +2
  1: 1d9a6a118fd1 [mq]: 2.patch - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  popping 2.patch
  now at: 1.patch
  ==== qnew -d -m
  Date: 6 0
  
  Three
  
  1: 2a9ef0bdefba Three - test - 6.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qref
  adding 3
  Date: 6 0
  
  Three
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  1: 7f19ad9eea7b Three - test - 6.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qref -m
  Date: 6 0
  
  Drei
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  1: 7ff7377793e3 Drei - test - 6.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qref -d
  Date: 7 0
  
  Drei
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  1: d89d3144f518 Drei - test - 7.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qref -d -m
  Date: 8 0
  
  Three (again)
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qnew -m
  adding 4
  Four
  
  diff -r ... 4
  --- /dev/null
  +++ b/4
  @@ -0,0 +1,1 @@
  +4
  2: 74ded07d166b Four - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -d
  Date: 9 0
  
  Four
  
  diff -r ... 4
  --- /dev/null
  +++ b/4
  @@ -0,0 +1,1 @@
  +4
  2: 1a651320cf8e Four - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  popping 4.patch
  now at: 3.patch
  ==== qnew with HG header
  popping 5.patch
  now at: 3.patch
  # HG changeset patch
  # Date 10 0
  2: d16a272220d2 imported patch 5.patch - test - 10.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== hg qref
  adding 5
  # HG changeset patch
  # Date 10 0
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  2: 5dbf69c07df9 [mq]: 5.patch - test - 10.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== hg qref -d
  # HG changeset patch
  # Date 11 0
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  2: 049de6af0c1d [mq]: 5.patch - test - 11.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qnew with plain header
  popping 6.patch
  now at: 5.patch
  now at: 6.patch
  Date: 12 0
  
  3: 8ad9ebc22b96 imported patch 6.patch - test
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== hg qref
  adding 6
  Date: 12 0
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  3: 038c46b02a56 [mq]: 6.patch - test - 12.00
  2: 049de6af0c1d [mq]: 5.patch - test - 11.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== hg qref -d
  Date: 13 0
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  3: 2785642ea4b4 [mq]: 6.patch - test - 13.00
  2: 049de6af0c1d [mq]: 5.patch - test - 11.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  popping 6.patch
  now at: 5.patch
  ==== qnew -u
  adding 6
  From: jane
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  3: a05a33f187ce [mq]: 6.patch - jane
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -d
  From: jane
  Date: 12 0
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  3: 5702c529dfe9 [mq]: 6.patch - jane
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  popping 6.patch
  now at: 5.patch
  ==== qnew -d
  adding 7
  Date: 13 0
  
  diff -r ... 7
  --- /dev/null
  +++ b/7
  @@ -0,0 +1,1 @@
  +7
  3: 8dd1eb8d4132 [mq]: 7.patch - test
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -u
  From: john
  Date: 13 0
  
  diff -r ... 7
  --- /dev/null
  +++ b/7
  @@ -0,0 +1,1 @@
  +7
  3: 4f9d07369cc4 [mq]: 7.patch - john - 13.00
  2: 049de6af0c1d [mq]: 5.patch - test - 11.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qnew
  adding 8
  diff -r ... 8
  --- /dev/null
  +++ b/8
  @@ -0,0 +1,1 @@
  +8
  4: 868b62f09492 [mq]: 8.patch - test
  3: 4f9d07369cc4 [mq]: 7.patch - john
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -u -d
  From: john
  Date: 14 0
  
  diff -r ... 8
  --- /dev/null
  +++ b/8
  @@ -0,0 +1,1 @@
  +8
  4: b1e878ae55b9 [mq]: 8.patch - john
  3: 4f9d07369cc4 [mq]: 7.patch - john
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  popping 8.patch
  now at: 7.patch
  ==== qnew -m
  adding 9
  Nine
  
  diff -r ... 9
  --- /dev/null
  +++ b/9
  @@ -0,0 +1,1 @@
  +9
  4: 7251936ac2bf Nine - test
  3: 4f9d07369cc4 [mq]: 7.patch - john
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -u -d
  From: john
  Date: 15 0
  
  Nine
  
  diff -r ... 9
  --- /dev/null
  +++ b/9
  @@ -0,0 +1,1 @@
  +9
  4: a0de5bf6e9f7 Nine - john
  3: 4f9d07369cc4 [mq]: 7.patch - john
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  popping 9.patch
  now at: 7.patch
  ==== qpop -a / qpush -a
  popping 7.patch
  popping 5.patch
  popping 3.patch
  popping 1.patch
  patch queue now empty
  applying 1.patch
  applying 3.patch
  applying 5.patch
  applying 7.patch
  now at: 7.patch
  3: d26a5b7ffce1 imported patch 7.patch - john - 13.00
  2: dda6cf77060a imported patch 5.patch - test - 11.00
  1: 25e32d66c8c7 Three (again) - test - 8.00
  0: e5011c0211fe imported patch 1.patch - test - 4.00
  $ rm -r sandbox

======= hg headers

  $ echo "plain=false" >> $HGRCPATH
  $ mkdir sandbox
  $ (cd sandbox ; runtest)
  ==== init
  ==== qnew -d
  # HG changeset patch
  # Date 3 0
  # Parent 
  
  0: 758bd2596a39 [mq]: 1.patch - test - 3.00
  ==== qref
  adding 1
  # HG changeset patch
  # Date 3 0
  # Parent 
  
  diff -r ... 1
  --- /dev/null
  +++ b/1
  @@ -0,0 +1,1 @@
  +1
  0: 8c640e9949a8 [mq]: 1.patch - test - 3.00
  ==== qref -d
  # HG changeset patch
  # Date 4 0
  # Parent 
  
  diff -r ... 1
  --- /dev/null
  +++ b/1
  @@ -0,0 +1,1 @@
  +1
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qnew
  adding 2
  # HG changeset patch
  # Parent 
  
  diff -r ... 2
  --- /dev/null
  +++ b/2
  @@ -0,0 +1,1 @@
  +2
  1: fc7e8a2f6499 [mq]: 2.patch - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -d
  # HG changeset patch
  # Date 5 0
  # Parent 
  
  diff -r ... 2
  --- /dev/null
  +++ b/2
  @@ -0,0 +1,1 @@
  +2
  1: 1d9a6a118fd1 [mq]: 2.patch - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  popping 2.patch
  now at: 1.patch
  ==== qnew -d -m
  # HG changeset patch
  # Date 6 0
  # Parent 
  Three
  
  1: 2a9ef0bdefba Three - test - 6.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qref
  adding 3
  # HG changeset patch
  # Date 6 0
  # Parent 
  Three
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  1: 7f19ad9eea7b Three - test - 6.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qref -m
  # HG changeset patch
  # Date 6 0
  # Parent 
  Drei
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  1: 7ff7377793e3 Drei - test - 6.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qref -d
  # HG changeset patch
  # Date 7 0
  # Parent 
  Drei
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  1: d89d3144f518 Drei - test - 7.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qref -d -m
  # HG changeset patch
  # Date 8 0
  # Parent 
  Three (again)
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qnew -m
  adding 4
  # HG changeset patch
  # Parent 
  Four
  
  diff -r ... 4
  --- /dev/null
  +++ b/4
  @@ -0,0 +1,1 @@
  +4
  2: 74ded07d166b Four - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -d
  # HG changeset patch
  # Date 9 0
  # Parent 
  Four
  
  diff -r ... 4
  --- /dev/null
  +++ b/4
  @@ -0,0 +1,1 @@
  +4
  2: 1a651320cf8e Four - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  popping 4.patch
  now at: 3.patch
  ==== qnew with HG header
  popping 5.patch
  now at: 3.patch
  # HG changeset patch
  # Date 10 0
  2: d16a272220d2 imported patch 5.patch - test - 10.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== hg qref
  adding 5
  # HG changeset patch
  # Date 10 0
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  2: 5dbf69c07df9 [mq]: 5.patch - test - 10.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== hg qref -d
  # HG changeset patch
  # Date 11 0
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  2: 049de6af0c1d [mq]: 5.patch - test - 11.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qnew with plain header
  popping 6.patch
  now at: 5.patch
  now at: 6.patch
  Date: 12 0
  
  3: 8ad9ebc22b96 imported patch 6.patch - test
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== hg qref
  adding 6
  Date: 12 0
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  3: 038c46b02a56 [mq]: 6.patch - test - 12.00
  2: 049de6af0c1d [mq]: 5.patch - test - 11.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== hg qref -d
  Date: 13 0
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  3: 2785642ea4b4 [mq]: 6.patch - test - 13.00
  2: 049de6af0c1d [mq]: 5.patch - test - 11.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  popping 6.patch
  now at: 5.patch
  ==== qnew -u
  adding 6
  # HG changeset patch
  # User jane
  # Parent 
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  3: a05a33f187ce [mq]: 6.patch - jane
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -d
  # HG changeset patch
  # User jane
  # Date 12 0
  # Parent 
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  3: 5702c529dfe9 [mq]: 6.patch - jane
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  popping 6.patch
  now at: 5.patch
  ==== qnew -d
  adding 7
  # HG changeset patch
  # Date 13 0
  # Parent 
  
  diff -r ... 7
  --- /dev/null
  +++ b/7
  @@ -0,0 +1,1 @@
  +7
  3: 8dd1eb8d4132 [mq]: 7.patch - test
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -u
  # HG changeset patch
  # User john
  # Date 13 0
  # Parent 
  
  diff -r ... 7
  --- /dev/null
  +++ b/7
  @@ -0,0 +1,1 @@
  +7
  3: 4f9d07369cc4 [mq]: 7.patch - john - 13.00
  2: 049de6af0c1d [mq]: 5.patch - test - 11.00
  1: b1b6b0fe0e6d Three (again) - test - 8.00
  0: 4a67dfeea974 [mq]: 1.patch - test - 4.00
  ==== qnew
  adding 8
  # HG changeset patch
  # Parent 
  
  diff -r ... 8
  --- /dev/null
  +++ b/8
  @@ -0,0 +1,1 @@
  +8
  4: 868b62f09492 [mq]: 8.patch - test
  3: 4f9d07369cc4 [mq]: 7.patch - john
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -u -d
  # HG changeset patch
  # User john
  # Date 14 0
  # Parent 
  
  diff -r ... 8
  --- /dev/null
  +++ b/8
  @@ -0,0 +1,1 @@
  +8
  4: b1e878ae55b9 [mq]: 8.patch - john
  3: 4f9d07369cc4 [mq]: 7.patch - john
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  popping 8.patch
  now at: 7.patch
  ==== qnew -m
  adding 9
  # HG changeset patch
  # Parent 
  Nine
  
  diff -r ... 9
  --- /dev/null
  +++ b/9
  @@ -0,0 +1,1 @@
  +9
  4: 7251936ac2bf Nine - test
  3: 4f9d07369cc4 [mq]: 7.patch - john
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  ==== qref -u -d
  # HG changeset patch
  # User john
  # Date 15 0
  # Parent 
  Nine
  
  diff -r ... 9
  --- /dev/null
  +++ b/9
  @@ -0,0 +1,1 @@
  +9
  4: a0de5bf6e9f7 Nine - john
  3: 4f9d07369cc4 [mq]: 7.patch - john
  2: 049de6af0c1d [mq]: 5.patch - test
  1: b1b6b0fe0e6d Three (again) - test
  0: 4a67dfeea974 [mq]: 1.patch - test
  popping 9.patch
  now at: 7.patch
  ==== qpop -a / qpush -a
  popping 7.patch
  popping 5.patch
  popping 3.patch
  popping 1.patch
  patch queue now empty
  applying 1.patch
  applying 3.patch
  applying 5.patch
  applying 7.patch
  now at: 7.patch
  3: d26a5b7ffce1 imported patch 7.patch - john - 13.00
  2: dda6cf77060a imported patch 5.patch - test - 11.00
  1: 25e32d66c8c7 Three (again) - test - 8.00
  0: e5011c0211fe imported patch 1.patch - test - 4.00
  $ rm -r sandbox
