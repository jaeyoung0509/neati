#!/usr/bin/env python3
"""Capture generated Svelte fixtures in an isolated agent-browser session.
Requires agent-browser and generated HTML; never runs native IPC or cleanup.
"""
import subprocess,json,pathlib,os,uuid,tempfile,shutil,sys
root=pathlib.Path(__file__).resolve().parent.parent
version=json.loads((root/'package.json').read_text())['version']
out=pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else root/'docs'/'validation'/f'temp-cleanup-{version}'
session='neati-cleanup-validation-'+uuid.uuid4().hex[:8]
profile=tempfile.mkdtemp(prefix='neati-validation-browser-')
base=['agent-browser','--session',session]
environment=dict(os.environ)
for key in ['AGENT_BROWSER_STATE','AGENT_BROWSER_SESSION_NAME','AGENT_BROWSER_AUTO_CONNECT','AGENT_BROWSER_PROVIDER']:
    environment.pop(key,None)
def run(*args,input=None):
    p=subprocess.run(base+list(args),input=input,text=True,capture_output=True,check=True,env=environment)
    return p.stdout.strip()
try:
    run('--profile',profile,'--allow-file-access','open','about:blank')
    selected={('initial','light',960),('empty','light',960),('empty','dark',960),('unavailable','dark',800),('privacy','light',800),('privacy','dark',800),('partial','light',960),('partial','dark',960),('retained','light',960),('available','dark',1280)}
    checks=[]
    for theme in ['light','dark']:
        run('set','media',theme,'reduced-motion')
        for width,height in [(800,560),(960,660),(1280,800)]:
            run('set','viewport',str(width),str(height))
            for scenario in ['initial','empty','unavailable','privacy','available','partial','retained']:
                run('open',(out/f'{scenario}-{theme}.html').as_uri())
                run('wait','--fn',"document.querySelector('main') !== null")
                data=json.loads(run('eval','--stdin',input="""(() => ({title:document.title, width:innerWidth, scrollWidth:document.documentElement.scrollWidth, height:Math.ceil(document.querySelector('main').getBoundingClientRect().height), reducedMotion:matchMedia('(prefers-reduced-motion: reduce)').matches, text:document.querySelector('main').innerText}))()"""))
                assert data['scrollWidth'] <= width,(scenario,theme,width,data)
                assert data['reducedMotion']
                if scenario in ['unavailable','privacy']:
                    assert '0 B' not in data['text']
                    assert 'Cleanup estimate unavailable' in data['text']
                if scenario=='empty': assert 'No cache items found in checked locations' in data['text']
                if scenario=='partial': assert 'Ready in checked locations' in data['text'] and '1 MB' in data['text']
                if scenario=='available': assert 'Ready to clean now' in data['text'] and 'Clean selected' in data['text']
                if (scenario,theme,width) in selected:
                    run('set','viewport',str(width),str(data['height']))
                    run('screenshot',str(out/f'{scenario}-{theme}-{width}.png'))
                    run('set','viewport',str(width),str(height))
                data.pop('text')
                data.update(scenario=scenario,theme=theme,viewportHeight=height)
                checks.append(data)
    (out/'browser-layout-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'{len(checks)} scenario/theme/width checks passed; {len(selected)} content-sized captures saved')
finally:
    subprocess.run(base+['close'],env=environment,capture_output=True,check=False)
    shutil.rmtree(profile,ignore_errors=True)
