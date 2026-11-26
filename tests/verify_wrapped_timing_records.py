from pathlib import Path
import subprocess,tempfile,codecs
runner=Path('docs/klbf/summarize_log.py')
host='30.7 fps, 32.5 ms/frame (1.3 preparing: copies 0.0 ms 0.0 MB, textures 1.1 ms 1.5 MB; 0.4 on the GPU; 0.0 copying back 0.0 MB), 1 draws/frame, dropped 0 without a host pipeline'
ffx='ffx: frame generation: 30.0 real fps, 60.0 fps presented on=true'
t='2026-11-25T21:17:31.235821Z  INFO '
prefix=t+'victory_gpu::host: '
fpre=t+'victory_gpu::ffx: '
def run(text,codec='utf-8',args=()):
 with tempfile.TemporaryDirectory() as d:
  p=Path(d)/'test.log'
  p.write_bytes(text.encode(codec) if codec!='utf-16-be' else codecs.BOM_UTF16_BE+text.encode(codec))
  cmd=['python',str(runner),str(p),'--breakdown',*args]
  r=subprocess.run(cmd,capture_output=True,text=True)
  return r.returncode,r.stdout
def eq(a,b,n):
 assert a==b,(n,a,b)
ref=run(prefix+host+'\n'+fpre+ffx)
assert ref[0]==0 and 'n=1' in ref[1] and 'on=true (1 samples' in ref[1]
cuts=0
for text,pre,reference in [(host,prefix,run(prefix+host)),(ffx,fpre,run(fpre+ffx))]:
 for i,c in enumerate(text):
  if c==' ':
   eq(run(pre+text[:i]+'\n'+text[i+1:]),reference,'split '+text[:i]);cuts+=1
for enc in ('utf-8','utf-8-sig','utf-16','utf-16-be'):
 eq(run(prefix+host.replace(' preparing:', '\npreparing:').replace('dropped ', '\ndropped ')+'\n'+fpre+ffx.replace(' on=true','\non=true'),enc),ref,enc)
eq(run(host+'\n'+ffx),ref,'bare rows')
eq(run(prefix+host+'\n'+fpre+ffx,args=['--lines','1','1']),run(prefix+host),'physical range')
eq(run(prefix+'8.1 fps, 100 ms/frame (4.0\n'+t+'other: interrupted\npreparing: x'),(1,''),'no cross-record synthesis')
eq(run(prefix+host.replace(' preparing:', '\npreparing:')+'\n',args=['--lines','1','1']),(1,''),'incomplete range')
eq(run(prefix+host+'\n'+t+'other: on=false\n'+fpre+ffx.replace(' on=true','')),run(prefix+host+'\n'+fpre+ffx.replace(' on=true','')),'no inherited framegen state')
print(cuts,'one-wrap positions; 4 encodings; bare records, truncation, range and boundaries passed')
