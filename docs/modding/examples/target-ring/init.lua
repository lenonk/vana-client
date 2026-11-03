-- Redrawn every frame, because the target moves.
events.on("frame", function()
  local t = game.target()
  if not t then
    draw.clear()
    return
  end
  -- y points down, so a smaller y is higher: lift the ring a little off the floor, and the text above the head.
  local feet = { x = t.pos.x, y = t.pos.y - 0.1, z = t.pos.z }
  draw.ring("ring", { at = feet, radius = 1.5, colour = { 255, 200, 60, 200 } })
  local above = { x = t.pos.x, y = t.pos.y - 2.4, z = t.pos.z }
  draw.text("distance", { at = above, text = ("%.1f yalms"):format(t.distance), colour = { 255, 255, 255 } })
end)
