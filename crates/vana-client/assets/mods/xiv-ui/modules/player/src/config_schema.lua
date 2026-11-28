-- Generated whitelist from owning mod settings + explicit native layout fields.
return {
  {key="on",type="bool",label="XIV Player Bars",help="Shows compact HP, MP and TP bars near the bottom of the screen.",default=false},
  {key="scale",type="int",label="Bar Scale",help="Scales the whole bar group. 100 matches the approved mockup.",default=100,min=50,max=200,step=10},
  {key="offset_y",type="int",label="Height",help="Moves the bars up (negative) or down from the default hotbar area (mid-screen anchor).",default=0,min=-400,max=1200,step=10},
  {key="offset_x",type="int",label="Horizontal Offset",help="",default=0,min=-800,max=800,step=10},
  {key="compact",type="bool",label="Compact Artwork",help="",default=false},
  {key="avoid_cast",type="bool",label="Avoid Cast/Attack Gauge Space",help="Uses the cast mod's actual reserved layout when available; no guessed behind-window geometry.",default=true},
}
