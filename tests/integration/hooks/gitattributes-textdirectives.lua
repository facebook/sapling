-- Note this should be added as a PerAddedOrModifiedFile hook.

function hook(ctx)
  if not ctx.file.path:match("%.gitattributes$") then
    return true
  end

  local content = ctx.file.content()
  if content:match("text.*=.*auto") then
    -- TODO: Include the line number and matching content in the error message.
    return false, "No text directives are authorized in .gitattributes. " ..
        "This is known to break sandcastle and developers' local clones."
  end

  return true
end
