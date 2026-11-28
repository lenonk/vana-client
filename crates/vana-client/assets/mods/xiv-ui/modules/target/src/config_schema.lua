-- Generated whitelist from owning mod settings + explicit native layout fields.
return {
  {key="on",type="bool",label="XIV Target Bar",help="Shows a compact target bar with status strip. Drag it to move it.",default=true},
  {key="width",type="int",label="Bar Width",help="",default=420,min=200,max=900,step=20},
  {key="scale",type="int",label="Target Scale",help="",default=100,min=50,max=200,step=10},
  {key="status_count",type="int",label="Visible Target Statuses",help="",default=12,min=1,max=32,step=1},
  {key="place_x",type="int",min=-2400,max=2400,step=4,default=0,label="X offset (screen pixels)",help="Owner-native geometry; not a renderer/window stacking workaround."},
  {key="place_y",type="int",min=-1600,max=1600,step=4,default=0,label="Y offset (screen pixels)",help="Owner-native geometry; not a renderer/window stacking workaround."},
}
