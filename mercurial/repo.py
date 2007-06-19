# repo.py - repository base classes for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

class RepoError(Exception):
    pass

class repository(object):
    def capable(self, name):
        '''tell whether repo supports named capability.
        return False if not supported.
        if boolean capability, return True.
        if string capability, return string.'''
        name_eq = name + '='
        for cap in self.capabilities:
            if name == cap:
                return True
            if cap.startswith(name_eq):
                return cap[len(name_eq):]
        return False
