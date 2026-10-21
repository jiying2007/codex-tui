import json
import os
from pathlib import Path
import subprocess

BASE = 'd74020ecd69b19ed83d2342b3d99497b800e68d0'
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip() == BASE
path = Path('release/v1.4-plan.json')
plan = json.loads(path.read_text())
changes = {}
for source in sorted(Path('src').rglob('*.rs')):
    name = source.as_posix()
    old = subprocess.run(['git','show',BASE+':'+name],capture_output=True)
    if old.returncode == 0 and old.stdout == source.read_bytes():
        continue
    count = len(source.read_text().splitlines())
    previous = plan['moduleRatchet'].get(name)
    plan['moduleRatchet'][name] = count
    changes[name] = {'previousCeiling': previous, 'reviewedCeiling': count}
assert changes
plan.setdefault('ratchetReviews',[]).append({
    'date':'2026-10-21','kind':'user-authorized-lean-control-plane-development',
    'scope':'New scope in release/v1.5-lean-plan.json: attention, daily navigation, truthful scoped headless and bounded List formatting. Existing safety and maintenance-only authorities remain. Only changed/new modules have reviewed ceilings.',
    'modules':sorted(changes),'ceilingChanges':changes})
path.write_text(json.dumps(plan,ensure_ascii=False,indent=2)+'\n')
subprocess.run(['git','add','--','src','tests','docs','scripts','release','.github/workflows','README.md','README.zh-CN.md'],check=True)
tree = subprocess.check_output(['git','write-tree'],text=True).strip()
result = {'baseSha':BASE,'candidateTree':tree,'moduleReviews':changes}
out = Path(os.environ['RUNNER_TEMP'])/'lean-results'
out.mkdir(exist_ok=True)
(out/'candidate-before-tests.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
