from pathlib import Path
from lean_patch_support import replace

# Ratatui stores continuation cells after wide glyphs. Compare visible glyphs, not padding bytes.
replace('tests/lean_navigation.rs', '.content.iter().map(|cell| cell.symbol()).collect::<String>();', '.content.iter().flat_map(|cell| cell.symbol().chars()).filter(|character| !character.is_whitespace()).collect::<String>();')
replace('tests/palette_convergence.rs', 'assert_eq!(app.command_palette_choice(), Some(Command::Search));', 'assert_eq!(app.command_palette_choice(), Some(Command::Today));', 2)
replace('tests/palette_convergence.rs', '    for primary in [\n        Command::Search,', '    for primary in [\n        Command::Today,\n        Command::Projects,\n        Command::ReviewQueue,\n        Command::Search,')
p=Path('src/runtime_palette/intent_tests.rs'); p.write_text(p.read_text()+r'''

#[test]
fn delivery_palette_intent_cannot_follow_another_selection_or_checkout() {
    let mut app = fixture();
    choose(&mut app, Command::Delivery);
    app.selected = 1;
    refused(&mut app);
    let mut app = fixture();
    choose(&mut app, Command::Delivery);
    let mut threads = app.threads.clone();
    threads[0].metadata.cwd = "/different/delivery-repository".into();
    reduce(&mut app, Action::ReplaceThreads(threads));
    refused(&mut app);
}

#[test]
fn project_navigation_is_global_after_selected_thread_disappears() {
    let mut app = fixture();
    choose(&mut app, Command::Projects);
    reduce(&mut app, Action::ReplaceThreads(vec![]));
    assert!(enter(&mut app).is_empty());
    assert_eq!(app.active_saved_view().id, "builtin:projects");
}
''')
for path,text in [('README.md','\nThe 2026-10-21 user-authorized next-development scope is the bounded Lean Mission Control plan linked above. Historical v1.4 completion remains a checkpoint, not a claim that this new scope is already Stable-qualified. No upstream authority or release gate is relaxed.\n'),('README.zh-CN.md','\n2026-10-21 用户授权的下一开发范围以本页精简任务中心方案为准。既有 v1.4 完成记录保留为检查点，不代表新增范围已经通过稳定发布资格；上游权威与发布门禁均不放宽。\n')]:
    p=Path(path); s=p.read_text(); marker='<!-- docs-section: status -->'; assert marker in s; p.write_text(s.replace(marker,marker+'\n'+text,1))
