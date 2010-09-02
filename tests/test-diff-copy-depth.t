  $ for i in aaa zzz; do
  >     hg init t
  >     cd t
  > 
  >     echo
  >     echo "-- With $i"
  > 
  >     touch file
  >     hg add file
  >     hg ci -m "Add"
  > 
  >     hg cp file $i
  >     hg ci -m "a -> $i"
  > 
  >     hg cp $i other-file
  >     echo "different" >> $i
  >     hg ci -m "$i -> other-file"
  > 
  >     hg cp other-file somename
  > 
  >     echo "Status":
  >     hg st -C
  >     echo
  >     echo "Diff:"
  >     hg diff -g
  > 
  >     cd ..
  >     rm -rf t
  > done
  
  -- With aaa
  Status:
  A somename
    other-file
  
  Diff:
  diff --git a/other-file b/somename
  copy from other-file
  copy to somename
  
  -- With zzz
  Status:
  A somename
    other-file
  
  Diff:
  diff --git a/other-file b/somename
  copy from other-file
  copy to somename


