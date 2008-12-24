from mercurial import node

import hg_delta_editor


def tags_from_tag_info(repo):
    hg_editor = hg_delta_editor.HgChangeReceiver(repo=repo)
    for tag, source in hg_editor.tags.iteritems():
        source_ha = hg_editor.get_parent_revision(source[1]+1, source[0])
        yield 'tag/%s'%tag, node.hex(source_ha)


def generate_repo_class(ui, repo):

    class svntagrepo(repo.__class__):
        def tags(self):
            tags = dict((k, node.bin(v))
                        for k,v in tags_from_tag_info(self))
            hg_tags = super(svntagrepo, self).tags()
            tags.update(hg_tags)
            return tags

    return svntagrepo
