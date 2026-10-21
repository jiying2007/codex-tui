from pathlib import Path
from lean_patch_support import replace

replace('src/app/mission.rs', '        self.view = View::Registry;\n        self.input_mode', '        self.view = View::Registry;\n        self.show_all_history = false;\n        self.filter.clear();\n        self.input_mode')
# Keep usage, runbook and actual parser aligned without making transcript search an alias of metadata search.
for path in ['docs/implementation/lean-mission-control.md','docs/zh-CN/implementation/lean-mission-control.md']:
    p=Path(path); s=p.read_text(); marker='<!-- docs-section: qualification -->'; assert marker in s
    text='\n```bash\npython3 scripts/release/productivity_report.py --template > observations.json\npython3 scripts/release/productivity_report.py observations.json\n```\n'
    if '/zh-CN/' in path:
        text+='\n填写实际候选 SHA、基线版本和匿名环境编号后，按脚本 schema 追加成对观察。空模板输出 NOT_MEASURED、退出 3；不生成伪造的效率收益。\n'
    else:
        text+='\nFill the actual candidate SHA, baseline version and anonymous environment ID, then append paired observations using the script schema. An empty template reports NOT_MEASURED and exits 3; it never invents productivity gains.\n'
    p.write_text(s.replace(marker, marker+'\n'+text,1))
