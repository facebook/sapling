hook = function (ctx)

  local tup_blacklist = {"^fbcode/tupperware/config/common/",
                         "^fbcode/tupperware/config/twcron/",
                         "^fbcode/tupperware/config/managed_containers/"}
  for _, pat in ipairs(tup_blacklist) do
    if string.match(ctx.file.path, pat) then
       return false,
        "File " .. ctx.file.path .. " is not in an allowed directory. ",
        "Commits to " .. pat .. " are only \z
         allowed in configerator (raw_configs/tupperware/config/...) \z
         and will be synced to fbcode. \z
         If you have any questions ask in tupperware@fb group."
     end
  end

  if string.match(ctx.file.path, "^fbcode/dataswarm%-pipelines/") then
    return false,
      "File " .. ctx.file.path .. " is in fbcode/dataswarm-pipelines/",
      "This directory is currently mirrored from dataswarm-svn repo and \z
       should not be directly committed to. \z
       Contact the Source Control @ FB group with questions and see \z
       https://fburl.com/w3oyww9c for more detail."
  end

  local allowed_patterns = {"^fbcode", "^fbandroid", "^fbobjc", "^tools",
                            "^xplat", "^%.[^/]*$"}
  for _, pat in ipairs(allowed_patterns) do
    if string.match(ctx.file.path, pat) then
      return true
    end
  end

  return
    false,
    "File " .. ctx.file.path .. " is not in an allowed directory. ",
    "Please make sure your files are under only the allowed top-level \z
     directories (fbandroid, fbcode, fbobjc, tools, xplat). \z
     Contact the Source Control @ FB group with questions and \z
     see https://fb.quip.com/LFJUAQCdMYFk for previous discussion."
end
