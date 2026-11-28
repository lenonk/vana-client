-- Generated whitelist from owning mod settings + explicit native layout fields.
return {
  {key="on",type="bool",label="XIV Focus and Last Target",help="",default=true},
  {key="width",type="int",label="Bar Width",help="",default=300,min=160,max=600,step=20},
  {key="scale",type="int",min=50,max=200,step=10,default=100,label="Own scale (%)",help="Owner-native geometry; not a renderer/window stacking workaround."},
  {key="offset_x",type="int",min=-1200,max=1200,step=4,default=0,label="Focus X offset",help="Owner-native geometry; not a renderer/window stacking workaround."},
  {key="offset_y",type="int",min=-800,max=800,step=4,default=0,label="Focus Y offset",help="Owner-native geometry; not a renderer/window stacking workaround."},
  {key="last_x",type="int",min=-1200,max=1200,step=4,default=0,label="Last-target X offset",help="Owner-native geometry; not a renderer/window stacking workaround."},
  {key="last_y",type="int",min=-800,max=800,step=4,default=0,label="Last-target Y offset",help="Owner-native geometry; not a renderer/window stacking workaround."},
}
