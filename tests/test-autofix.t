#require parso

  $ cat > a.py << EOF
  > from testutil.autofix import eq
  > eq(1 + 2, 0)
  > eq(list(range(3)), None)
  > eq("\n".join(map(str,range(3))), None)
  > EOF

  $ python a.py 2>&1 | tail -1
  AssertionError: 3 != 0

  $ python a.py --fix
  $ cat a.py
  from testutil.autofix import eq
  eq(1 + 2, 3)
  eq(list(range(3)), [0, 1, 2])
  eq("\n".join(map(str,range(3))), r"""
      0
      1
      2""")
