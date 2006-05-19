# packagescan.py - Helper module for identifing used modules.
# Used for the py2exe distutil.
# This module must be the first mercurial module imported in setup.py
#
# Copyright 2005 Volker Kleinfeld <Volker.Kleinfeld@gmx.de>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
import glob
import os
import sys
import ihooks

# Install this module as fake demandload module
sys.modules['mercurial.demandload'] = sys.modules[__name__]

# Requiredmodules contains the modules imported by demandload.
# Please note that demandload can be invoked before the 
# mercurial.packagescan.scan method is invoked in case a mercurial
# module is imported.
requiredmodules = {} 
def demandload(scope, modules):
    """ fake demandload function that collects the required modules """
    for m in modules.split():
        mod = None
        try:
            module, submodules = m.split(':')
            submodules = submodules.split(',')
        except:
            module = m
            submodules = []
        mod = __import__(module, scope, scope, submodules)
        scope[module] = mod
        requiredmodules[mod.__name__] = 1

def scan(libpath,packagename):
    """ helper for finding all required modules of package <packagename> """
    # Use the package in the build directory
    libpath = os.path.abspath(libpath)
    sys.path.insert(0,libpath)
    packdir = os.path.join(libpath,packagename)
    # A normal import would not find the package in
    # the build directory. ihook is used to force the import.
    # After the package is imported the import scope for
    # the following imports is settled.
    p = importfrom(packdir)
    globals()[packagename] = p
    sys.modules[packagename] = p
    # Fetch the python modules in the package
    cwd = os.getcwd()
    os.chdir(packdir)
    pymodulefiles = glob.glob('*.py')
    extmodulefiles = glob.glob('*.pyd')
    os.chdir(cwd)
    # Import all python modules and by that run the fake demandload
    for m in pymodulefiles:
        if m == '__init__.py': continue
        tmp = {}
        mname,ext = os.path.splitext(m)
        fullname = packagename+'.'+mname
        __import__(fullname,tmp,tmp)
        requiredmodules[fullname] = 1
    # Import all extension modules and by that run the fake demandload
    for m in extmodulefiles:
        tmp = {}
        mname,ext = os.path.splitext(m)
        fullname = packagename+'.'+mname
        __import__(fullname,tmp,tmp)
        requiredmodules[fullname] = 1

def getmodules():
    return requiredmodules.keys()

def importfrom(filename):
    """
    import module/package from a named file and returns the module.
    It does not check on sys.modules or includes the module in the scope.
    """
    loader = ihooks.BasicModuleLoader()
    path, file = os.path.split(filename)
    name, ext  = os.path.splitext(file)
    m = loader.find_module_in_dir(name, path)
    if not m:
        raise ImportError, name
    m = loader.load_module(name, m)
    return m
