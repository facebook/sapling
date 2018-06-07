#require no-windows

Test maxrss() by allocating 300 MB:

  >>> from mercurial import util
  >>> MEGABYTE = 1024 ** 2
  >>> start = util.getmaxrss()
  >>> a = bytearray(300 * MEGABYTE)
  >>> assert start + 300 * MEGABYTE <= util.getmaxrss() < start + 400 * MEGABYTE
  >>> a = None
  >>> assert start + 300 * MEGABYTE <= util.getmaxrss() < start + 400 * MEGABYTE
