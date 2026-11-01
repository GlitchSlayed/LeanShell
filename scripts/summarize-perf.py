import csv,json,statistics,math
from pathlib import Path
root=Path('target/perf')
def stats(v):
 if not v:return 'unavailable'
 v=sorted(v);return f'{statistics.median(v):.3f} / {v[math.ceil(.95*len(v))-1]:.3f} / {max(v):.3f} (n={len(v)})'
lines=['# Wisp release performance','','All cells: median / p95 / max. Latencies and intervals in ms. Release launcher demo with nightly,perf-trace. Three independent runs per scenario/DPI. No synthetic input. First-frame endpoint is successful Direct2D EndDraw submission, not physical scanout. The demo calls the show handler directly, bypassing WM_HOTKEY delivery and foreground activation. CPU is startup-inclusive one-core percent sampled after only 1 second; wakeups are dock poll callbacks per second, not kernel context switches. Working set is peak sampled MiB. Startup is app::run entry to ready, excluding OS loader.','','| DPI | Metric | Before | After |','|---|---|---|---|']
for dpi in (96,120):
 values={}
 for phase in ('baseline','after','cached'):
  for scenario in ('hidden','morph','typing','slide','idle-hidden','idle-shown'):
   files=list(root.glob(f'{phase}-{dpi}-{scenario}-*.csv'));rows=[r for p in files for r in csv.DictReader(p.open())]
   for metric in ('hotkey_to_content','key_to_content','show','search','paint','morph','slide','status','surfaces','ready'):
    v=[float(r['start_ms'] if metric=='ready' else r['duration_ms']) for r in rows if r['metric']==metric and (metric!='morph' or float(r['duration_ms']) > 0.1)]
    if v:values[phase,scenario,metric]=stats(v)
   for kind in ('morph','slide'):
    intervals=[]
    for p in files:
     prev=None
     for r in csv.DictReader(p.open()):
      if r['metric']==kind+'_begin':prev=None
      if r['metric']==kind+'_frame':
       t=float(r['start_ms'])
       if prev is not None:intervals.append(t-prev)
       prev=t
    if intervals:values[phase,scenario,kind+'_interval']=stats(intervals)+f'; >16.7: {sum(x>16.7 for x in intervals)}'
   if files:values[phase,scenario,'poll_hz']=stats([sum(r['metric']=='dock_poll' for r in csv.DictReader(p.open()))/3.5 for p in files])
  proc=root/f'{phase}-process.jsonl'
  if proc.exists():
   data=[json.loads(r) for r in proc.read_text().splitlines()]
   for scenario in ('idle-hidden','idle-shown','hidden'):
    for key in ('steady_cpu_one_core_pct','peak_sampled_ws_mb'):
     values[phase,scenario,key]=stats([r[key] for r in data if r['dpi']==dpi and r['scenario']==scenario])
 for scenario,metric in [('hidden','hotkey_to_content'),('morph','hotkey_to_content'),('typing','key_to_content'),('typing','search'),('morph','morph'),('morph','morph_interval'),('slide','slide'),('slide','slide_interval'),('idle-hidden','steady_cpu_one_core_pct'),('idle-shown','steady_cpu_one_core_pct'),('idle-hidden','poll_hz'),('idle-shown','poll_hz'),('hidden','ready'),('hidden','peak_sampled_ws_mb'),('idle-shown','status'),('hidden','surfaces')]:
  label = 'startup_window_cpu_one_core_pct' if metric == 'steady_cpu_one_core_pct' else metric
  lines.append(f'| {dpi} | {scenario}: {label} | {values.get(("baseline",scenario,metric),"unavailable")} | {values.get(("cached",scenario,metric),values.get(("after",scenario,metric),"unavailable"))} |')
lines+=['','Raw data: `*-*.csv`, `*-process.jsonl`, and `*.log` alongside this report. `scripts/measure-perf.ps1` collects data; `scripts/summarize-perf.py` generates this table.','']
lines += ['', '## Layout-cache experiment', '', '| DPI | Morph work, software | Morph work, cached |', '|---|---|---|']
for dpi in (96,120):
 def work(phase):
  return stats([float(r['duration_ms']) for p in root.glob(f'{phase}-{dpi}-morph-*.csv') for r in csv.DictReader(p.open()) if r['metric']=='morph' and float(r['duration_ms']) > .1])
 lines.append(f'| {dpi} | {work("after")} | {work("cached")} |')
settled = root/'settled-process.jsonl'
if settled.exists():
 data=[json.loads(r) for r in settled.read_text().splitlines()]
 lines += ['', '## Settled idle (candidate only)', '', '15 second runs; CPU samples start after 10 seconds. One-core CPU percent. No comparable long-duration baseline was collected.', '', '| DPI | State | CPU median / p95 / max |', '|---|---|---|']
 for dpi in (96,120):
  for scenario in ('idle-hidden','idle-shown'):
   lines.append(f"| {dpi} | {scenario} | {stats([r['steady_cpu_one_core_pct'] for r in data if r['dpi']==dpi and r['scenario']==scenario])} |")
notes = root/'notes.md' 
if notes.exists(): lines.append(notes.read_text())
(root/'report.md').write_text('\n'.join(lines))
