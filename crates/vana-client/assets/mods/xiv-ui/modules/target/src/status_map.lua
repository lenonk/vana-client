-- Aggregation only: all leaves are loaded in separate entry slices.
-- budget_data/* tables: vana-client action/status facts (in-repo compile; sharded for M18 budget).
local out={actions={ability={},pet={},spell={},ws={}},ends={}}
for _,r in ipairs(require('src.budget_data.src_status_map_000')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_001')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_002')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_003')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_004')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_005')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_006')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_007')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_008')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_009')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_010')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_011')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_012')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_013')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_014')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_015')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_016')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_017')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_018')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_019')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_020')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_021')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_022')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_023')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_024')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_025')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_026')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_027')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_028')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_029')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_030')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_031')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_032')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_033')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
for _,r in ipairs(require('src.budget_data.src_status_map_034')) do if r[1]=='actions' then out.actions[r[2]][r[3]]=r[4] else out.ends[r[2]]=r[3] end end
return out
