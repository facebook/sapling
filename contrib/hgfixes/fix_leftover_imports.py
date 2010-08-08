"Fixer that translates some APIs ignored by the default 2to3 fixers."

# FIXME: This fixer has some ugly hacks. Its main design is based on that of
# fix_imports, from lib2to3. Unfortunately, the fix_imports framework only
# changes module names "without dots", meaning it won't work for some changes
# in the email module/package. Thus this fixer was born. I believe that with a
# bit more thinking, a more generic fixer can be implemented, but I'll leave
# that as future work.

from lib2to3.fixer_util import Name
from lib2to3.fixes import fix_imports

# This maps the old names to the new names. Note that a drawback of the current
# design is that the dictionary keys MUST have EXACTLY one dot (.) in them,
# otherwise things will break. (If you don't need a module hierarchy, you're
# better of just inherit from fix_imports and overriding the MAPPING dict.)

MAPPING = {'email.Utils': 'email.utils',
           'email.Errors': 'email.errors',
           'email.Header': 'email.header',
           'email.Parser': 'email.parser',
           'email.Encoders': 'email.encoders',
           'email.MIMEText': 'email.mime.text',
           'email.MIMEBase': 'email.mime.base',
           'email.Generator': 'email.generator',
           'email.MIMEMultipart': 'email.mime.multipart',
}

def alternates(members):
    return "(" + "|".join(map(repr, members)) + ")"

def build_pattern(mapping=MAPPING):
    packages = {}
    for key in mapping:
        # What we are doing here is the following: with dotted names, we'll
        # have something like package_name <trailer '.' module>. Then, we are
        # making a dictionary to copy this structure. For example, if
        # mapping={'A.B': 'a.b', 'A.C': 'a.c'}, it will generate the dictionary
        # {'A': ['b', 'c']} to, then, generate something like "A <trailer '.'
        # ('b' | 'c')".
        name = key.split('.')
        prefix = name[0]
        if prefix in packages:
            packages[prefix].append(name[1:][0])
        else:
            packages[prefix] = name[1:]

    mod_list = ' | '.join(["'%s' '.' ('%s')" %
        (key, "' | '".join(packages[key])) for key in packages])
    mod_list = '(' + mod_list + ' )'
    bare_names = alternates(mapping.keys())

    yield """name_import=import_name< 'import' module_name=dotted_name< %s > >
          """ % mod_list

    yield """name_import=import_name< 'import'
            multiple_imports=dotted_as_names< any*
            module_name=dotted_name< %s >
            any* >
            >""" % mod_list

    packs = ' | '.join(["'%s' trailer<'.' ('%s')>" % (key,
               "' | '".join(packages[key])) for key in packages])

    yield "power< package=(%s) trailer<'.' any > any* >" % packs

class FixLeftoverImports(fix_imports.FixImports):
    # We want to run this fixer after fix_import has run (this shouldn't matter
    # for hg, though, as setup3k prefers to run the default fixers first)
    mapping = MAPPING

    def build_pattern(self):
        return "|".join(build_pattern(self.mapping))

    def transform(self, node, results):
        # Mostly copied from fix_imports.py
        import_mod = results.get("module_name")
        if import_mod:
            try:
                mod_name = import_mod.value
            except AttributeError:
                # XXX: A hack to remove whitespace prefixes and suffixes
                mod_name = str(import_mod).strip()
            new_name = self.mapping[mod_name]
            import_mod.replace(Name(new_name, prefix=import_mod.prefix))
            if "name_import" in results:
                # If it's not a "from x import x, y" or "import x as y" import,
                # marked its usage to be replaced.
                self.replace[mod_name] = new_name
            if "multiple_imports" in results:
                # This is a nasty hack to fix multiple imports on a line (e.g.,
                # "import StringIO, urlparse"). The problem is that I can't
                # figure out an easy way to make a pattern recognize the keys of
                # MAPPING randomly sprinkled in an import statement.
                results = self.match(node)
                if results:
                    self.transform(node, results)
        else:
            # Replace usage of the module.
            # Now this is, mostly, a hack
            bare_name = results["package"][0]
            bare_name_text = ''.join(map(str, results['package'])).strip()
            new_name = self.replace.get(bare_name_text)
            prefix = results['package'][0].prefix
            if new_name:
                bare_name.replace(Name(new_name, prefix=prefix))
                results["package"][1].replace(Name(''))

