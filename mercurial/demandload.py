def demandload(scope, modules):
    class d:
        def __getattr__(self, name):
            mod = self.__dict__["mod"]
            scope = self.__dict__["scope"]
            scope[mod] = __import__(mod, scope, scope, [])
            return getattr(scope[mod], name)

    for m in modules.split():
        dl = d()
        dl.mod = m
        dl.scope = scope
        scope[m] = dl


