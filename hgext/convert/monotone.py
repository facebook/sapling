# monotone support for the convert extension

import os
import re
import time
from mercurial import util

from common import NoRepo, commit, converter_source, checktool

class monotone_source(converter_source):
    def __init__(self, ui, path=None, rev=None):
        converter_source.__init__(self, ui, path, rev)
        
        self.ui = ui
        self.path = path

         
        # regular expressions for parsing monotone output
        
        space    = r'\s*'
        name     = r'\s+"((?:[^"]|\\")*)"\s*'
        value    = name
        revision = r'\s+\[(\w+)\]\s*'
        lines    = r'(?:.|\n)+'
        
        self.dir_re      = re.compile(space + "dir"      + name)
        self.file_re     = re.compile(space + "file"     + name + "content" + revision)
        self.add_file_re = re.compile(space + "add_file" + name + "content" + revision)
        self.patch_re    = re.compile(space + "patch"    + name + "from" + revision + "to" + revision)
        self.rename_re   = re.compile(space + "rename"   + name + "to" + name)
        self.tag_re      = re.compile(space + "tag"      + name + "revision" + revision)
        self.cert_re     = re.compile(lines + space + "name" + name + "value" + value)

        attr = space + "file" + lines + space + "attr" + space
        self.attr_execute_re = re.compile(attr  + '"mtn:execute"' + space + '"true"')

        # cached data
        
        self.manifest_rev = None
        self.manifest = None
        self.files = None   
        self.dirs  = None     
        
        norepo = NoRepo("%s does not look like a monotone repo" % path)
        if not os.path.exists(path):
            raise norepo
        
        checktool('mtn')
        
        # test if there are are any revisions
        self.rev = None
        try :
            self.getheads()
        except :
            raise norepo        

        self.rev = rev

    
    def mtncmd(self, arg):
        cmdline = "mtn -d %s automate %s" % (util.shellquote(self.path), arg)
        self.ui.debug(cmdline, '\n')
        p = util.popen(cmdline)
        result = p.read()
        if p.close():
            raise IOError()
        return result
    
    def mtnloadmanifest(self, rev):
        if self.manifest_rev == rev:
            return
        self.manifest_rev = rev
        self.manifest = self.mtncmd("get_manifest_of %s" % rev).split("\n\n")
        
        manifest = self.manifest
        files = {}
        dirs = {}

        for e in manifest:
            m = self.file_re.match(e)
            if m:                
                attr = ""
                name = m.group(1)
                node = m.group(2)
                if self.attr_execute_re.match(e):
                    attr += "x"
                files[name] = (node, attr)
            m = self.dir_re.match(e)
            if m:
                dirs[m.group(1)] = True
        
        self.files = files
        self.dirs = dirs

    def mtnisfile(self, name, rev):
        # a non-file could be a directory or a deleted or renamed file
        self.mtnloadmanifest(rev)
        try :
            self.files[name]
            return True
        except KeyError:
            return False
            
    def mtnisdir(self, name, rev):
        self.mtnloadmanifest(rev)
        try :
            self.dirs[name]
            return True
        except KeyError:
            return False
    
    def mtngetcerts(self, rev):
        certs = {"author":"<missing>", "date":"<missing>",
            "changelog":"<missing>", "branch":"<missing>"}
        cert_list = self.mtncmd("certs %s" % rev).split("\n\n")
        for e in cert_list:
            m = self.cert_re.match(e)
            if m:
                certs[m.group(1)] = m.group(2)
        return certs
        
    def mtngetparents(self, rev):
        parents = self.mtncmd("parents %s" % rev).strip("\n").split("\n")
        p = []
        for x in parents:
            if len(x) >= 40: # blank revs have been seen otherwise
                p.append(x)
        return p

    def mtnrenamefiles(self, files, fromdir, todir):
        renamed = {}
        for tofile in files:
            suffix = tofile.lstrip(todir)
            if todir + suffix == tofile:
                renamed[tofile] = (fromdir + suffix).lstrip("/")
        return renamed

    
    # implement the converter_source interface:
    
    def getheads(self):
        if not self.rev or self.rev == "":
            return self.mtncmd("leaves").splitlines()
        else:
            return [self.rev]

    def getchanges(self, rev):
        revision = self.mtncmd("get_revision %s" % rev).split("\n\n")
        files = {}
        copies = {}
        for e in revision:
            m = self.add_file_re.match(e)
            if m:
                files[m.group(1)] = rev
            m = self.patch_re.match(e)
            if m:
                files[m.group(1)] = rev

            # Delete/rename is handled later when the convert engine
            # discovers an IOError exception from getfile,
            # but only if we add the "from" file to the list of changes.
            m = self.rename_re.match(e)
            if m:
                toname = m.group(2)
                fromname = m.group(1)
                if self.mtnisfile(toname, rev):
                    copies[toname] = fromname
                    files[toname] = rev
                    files[fromname] = rev
                if self.mtnisdir(toname, rev):
                    renamed = self.mtnrenamefiles(self.files, fromname, toname)
                    for tofile, fromfile in renamed.items():
                        self.ui.debug (("copying file in renamed dir from '%s' to '%s'" % (fromfile, tofile)), "\n")
                        files[tofile] = rev
                    for fromfile in renamed.values():
                        files[fromfile] = rev

        return (files.items(), copies)
        
    def getmode(self, name, rev):
        self.mtnloadmanifest(rev)
        try :
            node, attr = self.files[name]
            return attr
        except KeyError:
            return ""
        
    def getfile(self, name, rev):
        if not self.mtnisfile(name, rev):
            raise IOError() # file was deleted or renamed
        return self.mtncmd("get_file_of %s -r %s" % (util.shellquote(name), rev))
    
    def getcommit(self, rev):        
        certs   = self.mtngetcerts(rev)
        return commit(
            author=certs["author"],
            date=util.datestr(util.strdate(certs["date"], "%Y-%m-%dT%H:%M:%S")),
            desc=certs["changelog"],
            rev=rev,
            parents=self.mtngetparents(rev),
            branch=certs["branch"])

    def gettags(self):
        tags = {}
        for e in self.mtncmd("tags").split("\n\n"):
            m = self.tag_re.match(e)
            if m:
                tags[m.group(1)] = m.group(2)
        return tags

    def getchangedfiles(self, rev, i):
        # This function is only needed to support --filemap
        # ... and we don't support that
        raise NotImplementedError()
