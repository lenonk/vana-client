-- Generated whitelist from owning mod settings + explicit native layout fields.
return {
  {key="on",type="bool",label="XIV Party List",help="Draws a compact party frame on the left of the screen.",default=true},
  {key="offset_x",type="int",label="Left offset",help="Distance from the left edge of the screen.",default=16,min=0,max=1200,step=4},
  {key="offset_y",type="int",label="Top offset",help="Distance from the top of the screen.",default=120,min=0,max=1200,step=4},
  {key="scale",type="int",label="Party Scale",help="",default=100,min=50,max=150,step=10},
  {key="buff_count",type="int",label="Visible Main Party Statuses",help="",default=6,min=1,max=32,step=1},
  {key="range",type="int",label="Range Marker (yalms)",help="",default=21,min=5,max=30,step=1},
  {key="pets",type="bool",label="Show Observed Pets",help="",default=true},
  {key="hide_solo",type="bool",label="Hide Solo",help="",default=false},
  {key="hide_cutscene",type="bool",label="Hide During Events",help="",default=true},
  {key="align_bottom",type="bool",label="Bottom-align Party Rows",help="",default=false},
  {key="single_alliance",type="bool",label="Full-width Single Alliance",help="",default=false},
  {key="range_numbers",type="bool",label="Numeric Range",help="",default=false},
  {key="filter_whitelist",type="bool",label="Only Prioritised Statuses",help="",default=false},
}
