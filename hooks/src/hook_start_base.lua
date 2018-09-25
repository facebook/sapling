__hook_start_base = function(info, arg, setup)
     if hook == nil then
        error("no hook function")
     end
     local ctx = {}
     ctx.info=info
     setup(arg, ctx)
     io = nil
     os = nil
     acc, desc, long_desc = hook(ctx)
     if type(acc) ~= "boolean" then
        error("invalid hook return type")
     end
     if acc and desc ~= nil then
        error("failure description must only be set if hook fails")
     end
     if acc and long_desc ~= nil then
        error("failure long description must only be set if hook fails")
     end
     if desc ~= nil and type(desc) ~= "string" then
        error("invalid hook failure short description type")
     end
     if long_desc ~= nil and type(long_desc) ~= "string" then
        error("invalid hook failure long description type")
     end
     res = {acc, desc, long_desc}
     return res
end
