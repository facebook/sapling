import os, stat

def _mode_to_kind(mode):
    if stat.S_ISREG(mode): return stat.S_IFREG
    if stat.S_ISDIR(mode): return stat.S_IFDIR
    if stat.S_ISLNK(mode): return stat.S_IFLNK
    if stat.S_ISBLK(mode): return stat.S_IFBLK
    if stat.S_ISCHR(mode): return stat.S_IFCHR
    if stat.S_ISFIFO(mode): return stat.S_IFIFO
    if stat.S_ISSOCK(mode): return stat.S_IFSOCK
    return mode

def listdir(path, stat=False):
    '''listdir(path, stat=False) -> list_of_tuples

    Return a sorted list containing information about the entries
    in the directory.

    If stat is True, each element is a 3-tuple:

      (name, type, stat object)

    Otherwise, each element is a 2-tuple:

      (name, type)
    '''
    result = []
    prefix = path + os.sep
    names = os.listdir(path)
    names.sort()
    for fn in names:
        st = os.lstat(prefix + fn)
        if stat:
            result.append((fn, _mode_to_kind(st.st_mode), st))
        else:
            result.append((fn, _mode_to_kind(st.st_mode)))
    return result
