-- Target bar colours by entity kind (shared by target and focus panels).
local P = {
  enemy = { track = { 80, 15, 15, 255 }, fill = { 255, 186, 183, 255 }, border = { 255, 140, 140, 255 }, text = { 255, 186, 183, 255 } },
  player = { track = { 8, 45, 82, 255 }, fill = { 226, 255, 255, 255 }, border = { 80, 180, 250, 255 }, text = { 226, 255, 255, 255 } },
  npc = { track = { 70, 52, 10, 255 }, fill = { 255, 238, 170, 255 }, border = { 250, 208, 90, 255 }, text = { 255, 238, 170, 255 } },
  object = { track = { 42, 42, 46, 255 }, fill = { 214, 214, 220, 255 }, border = { 172, 172, 180, 255 }, text = { 214, 214, 220, 255 } },
}
return function(t)
  if t.kind == "player" or t.in_party or t.in_alliance or t.trust or t.pet then return P.player end
  if t.kind == "mob" then return P.enemy end
  if t.gauge == false or t.walks == false then return P.object end
  return P.npc
end