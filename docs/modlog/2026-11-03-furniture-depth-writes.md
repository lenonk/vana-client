## Furniture: blended meshes keep their depth writes (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch7.md item 23): furniture and ship models are drawn
  by FUN_101844c0 in the actor pass. Per mesh it sets ALPHABLENDENABLE from the mesh's bit-31 flag, but it never sets
  ZWRITEENABLE and never sets ZBIAS, so the blended meshes keep the pass's depth writes (on) and bias 0. There is no
  depth sort either: meshes go in file order, instances in actor order.
- The zone scenery draws (FUN_1017c9c0 / FUN_1017de20) do the opposite: a bit-31 mesh gets ALPHABLEND on and
  ZWRITE off. So retail's depth behaviour differs between scenery and furniture. This also answers MODLOG L4325 for
  scenery.
- Ours used the scenery blend pipeline for furniture (no depth write, bias -8). render/mod.rs now has furniture
  copies of the two blended classes with depth writes on and no bias, used for furniture and for ship models.
- Inferred: the depth compare. Retail leaves whatever the actor pass set; ours uses Less, as the opaque classes do.
