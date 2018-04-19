Test sortdictfilter.py which stabilizes dict output

  $ python $RUNTESTDIR/sortdictfilter.py << EOS
  > {'b': 3, 'a': {'d': 5, 'g': 6, 'c': 7}} - foobar - {11: ['x', 'a'], 1: None}
  > {'a': {'d': 5, 'c': 7, 'g': 6}, 'b': 3} - foobar - {1: None, 11: ['x', 'a']}
  > {not a valid dict, 2: 1, 1: 2} {{{'incomplete': True}
  > EOS
  {'a': {'c': 7, 'd': 5, 'g': 6}, 'b': 3} - foobar - {1: None, 11: ['x', 'a']}
  {'a': {'c': 7, 'd': 5, 'g': 6}, 'b': 3} - foobar - {1: None, 11: ['x', 'a']}
  {not a valid dict, 2: 1, 1: 2} {{{'incomplete': True}
