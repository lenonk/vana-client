-- Generated whitelist from owning mod settings + explicit native layout fields.
return {
  {key="on",type="bool",label="XIV Points Strip",help="",default=true},
  {key="mode",type="int",label="Display Mode: 1 Auto, 2 EXP, 3 Limit, 4 Capacity",help="Display only. Auto uses level/available data; the game's allocation switch is not exposed.",default=1,min=1,max=4,step=1},
  {key="scale",type="int",label="XP Strip Scale",help="",default=100,min=50,max=200,step=10},
  {key="offset_x",type="int",label="Horizontal Offset",help="",default=0,min=-800,max=800,step=10},
  {key="offset_y",type="int",label="Vertical Offset From Top",help="",default=6,min=0,max=800,step=1},
  {key="dark_thin",type="bool",label="Thin Dark/Glow Artwork",help="",default=true},
  {key="stats_text",type="bool",label="Observed Rate / Estimated Time Text",help="",default=false},
  {key="attach_vitals",type="bool",label="Attach To Player Bars",help="On: XP strip follows player bars; drag player to move both. Off: drag the XP strip.",default=false},
  {key="attach_below",type="bool",label="Attach Below Player Bars (off = above)",help="Only when Attach To Player Bars is on. Default below vitals.",default=true},
}
