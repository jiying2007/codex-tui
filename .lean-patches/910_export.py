import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess

BASE = 'd74020ecd69b19ed83d2342b3d99497b800e68d0'
BASE_TREE = 'a73172af52b900437dbc45052b2ba48d9163130c'
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip() == BASE
subprocess.run(['git','diff','--exit-code'],check=True)
out=Path(os.environ['RUNNER_TEMP'])/'lean-results'
previous=json.loads((out/'candidate-before-tests.json').read_text())
tree=subprocess.check_output(['git','write-tree'],text=True).strip()
assert tree == previous['candidateTree']
paths=subprocess.check_output(['git','diff','--cached','--name-only','-z'],text=False).split(b'\0')
files=[]
for raw in paths:
    if not raw: continue
    name=raw.decode('utf-8')
    data=subprocess.check_output(['git','show',':'+name])
    record=subprocess.check_output(['git','ls-files','--stage','--',name],text=True).split()
    mode, sha=record[:2]
    assert sha == hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest()
    files.append({'path':name,'mode':mode,'sha':sha,'content':base64.b64encode(data).decode()})
result={'baseSha':BASE,'baseTree':BASE_TREE,'candidateTree':tree,'files':files,
 'verification':'hosted-linux-cargo-tests-clippy-python-docs-architecture','stableReady':False,'publicationAllowed':False}
(out/'tree-data.json').write_text(json.dumps(result,ensure_ascii=False)+'\n')
(out/'candidate.patch').write_bytes(subprocess.check_output(['git','diff','--cached','--binary']))
(out/'candidate-stat.txt').write_text(subprocess.check_output(['git','diff','--cached','--stat'],text=True))
print(json.dumps({'candidateTree':tree,'files':[row['path'] for row in files],'stableReady':False,'publicationAllowed':False},indent=2))
