import os, shutil
import basestore, ioutil
from mercurial import util
from mercurial.node import hex

class remotefilelogcontentstore(basestore.basestore):
    def get(self, name, node):
        pass

    def add(self, name, node, data):
        raise Exception("cannot add content only to remotefilelog "
                        "contentstore")
