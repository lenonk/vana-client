-- Layout fields stored in storage.account.xiv_ui_notify_v1 (not in suite mod.toml).
-- Manifest keys (seconds, scale, reminders, invite offsets) live in mod.toml as notify_*.
return {
  {key="offset_x",type="int",label="Toast horizontal offset",help="Panel layout bucket; use Mods for timing/scale/reminders.",default=0,min=-600,max=600,step=10},
  {key="offset_y",type="int",label="Toast vertical offset",help="Panel layout bucket; use Mods for timing/scale/reminders.",default=0,min=-300,max=600,step=10},
}