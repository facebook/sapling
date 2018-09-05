#require no-windows

Test maxrss() by allocating 300 MB:

  >>> from mercurial import util
  >>> MEGABYTE = 1024 ** 2
  >>> AMOUNT = 300 * MEGABYTE
  >>> start = util.getmaxrss()
  >>> a = bytearray(300 * MEGABYTE)
  >>> assert start + AMOUNT <= util.getmaxrss() < start + AMOUNT * 2
  >>> a = None
  >>> assert start + AMOUNT <= util.getmaxrss() < start + AMOUNT * 2
