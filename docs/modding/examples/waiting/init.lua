commands.register("nearest", function()
  local best
  for _, e in ipairs(game.entities()) do
    if not e.me and e.targetable and (best == nil or e.distance < best.distance) then
      best = e
    end
  end
  if not best then
    chat.add("Nobody is near.")
    return
  end

  actions.target(best.id)

  -- The target changes after this frame: wait for it, for 2 seconds at most.
  local function targeted()
    local t = game.target()
    return t ~= nil and t.id == best.id
  end
  wait_until(targeted, 2)

  if targeted() then
    chat.add(("Targeted %s, %.1f yalms away."):format(best.name, best.distance))
  else
    chat.add("Could not target " .. best.name .. ".")
  end
end)
