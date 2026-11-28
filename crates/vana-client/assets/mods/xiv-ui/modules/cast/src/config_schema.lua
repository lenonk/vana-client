-- Generated whitelist from owning mod settings + explicit native layout fields.
return {
  {key="offset_x",type="int",min=-1200,max=1200,step=4,default=0,label="Horizontal HUD offset",help="Native gauge origin, no window stacking workaround."},
  {key="on",type="bool",label="XIV Cast Bar",help="Shows your casting progress as a gold-rimmed bar.",default=true},
  {key="width",type="int",label="Width",help="Width of the cast track in UI units. 180 matches the approved mockup.",default=180,min=100,max=500,step=10},
  {key="offset_y",type="int",label="Vertical offset",help="Moves the bar up or down from its default position.",default=0,min=-400,max=400,step=4},
  {key="cast",type="bool",label="Show Casting",help="",default=true},
  {key="swing",type="bool",label="Show Estimated Melee Timing",help="",default=true},
  {key="ranged",type="bool",label="Show Estimated Ranged Timing",help="",default=true},
  {key="swing_text",type="bool",label="Show Interval/Hit Labels",help="",default=false},
}
