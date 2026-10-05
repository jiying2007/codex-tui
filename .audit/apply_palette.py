import pathlib, shutil, sys

def replace(path, before, after):
    p = pathlib.Path(path)
    content = p.read_text()
    assert content.count(before) == 1, (path, before, content.count(before))
    p.write_text(content.replace(before, after))

if sys.argv[1] == 'tests':
    pathlib.Path('src/runtime_palette').mkdir(exist_ok=True)
    shutil.copyfile('.audit/intent_tests.rs', 'src/runtime_palette/intent_tests.rs')
    with open('src/runtime_palette.rs', 'a') as f:
        f.write('\n#[cfg(test)]\n#[path = "runtime_palette/intent_tests.rs"]\nmod intent_tests;\n')
else:
    shutil.copyfile('.audit/palette_intent.rs', 'src/app/palette_intent.rs')
    replace('src/app.rs', 'pub(crate) mod palette;\n', 'pub(crate) mod palette;\nmod palette_intent;\n')
    replace('src/app.rs', '    command_palette_items: Vec<Command>,\n', '    command_palette_items: Vec<Command>,\n    command_palette_intent: Option<palette_intent::PaletteIntent>,\n')
    replace('src/app.rs', '            command_palette_items: vec![],\n', '            command_palette_items: vec![],\n            command_palette_intent: None,\n')
    replace('src/app/palette.rs', '        self.command_palette_items = choices;\n', '        self.command_palette_intent = Some(self.palette_intent());\n        self.command_palette_items = choices;\n')
    replace('src/app/palette.rs', '        self.command_palette_items.clear();\n', '        self.command_palette_items.clear();\n        self.command_palette_intent = None;\n')
    replace('src/runtime_palette.rs', '            let Some(choice) = app.command_palette_choice() else {', '            let Some(choice) = app.take_command_palette_choice() else {')
    replace('src/runtime_palette.rs', '            reduce(app, Action::CloseCommandPalette);\n            handle_command(app, choice)', '            handle_command(app, choice)')
    replace('release/v1.4-plan.json', '    "src/app/palette.rs": 420,\n', '    "src/app/palette.rs": 420,\n    "src/app/palette_intent.rs": 180,\n    "src/runtime_palette/intent_tests.rs": 280,\n')
    shutil.rmtree('.audit')
    pathlib.Path('.github/workflows/audit-palette-intent.yml').unlink()
