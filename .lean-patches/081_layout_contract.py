from pathlib import Path
from lean_patch_support import replace

# Keep backend provenance at the start of the title even while searching all history.
replace('src/ui.rs', 'tr(app, "Mission Control · Today [F1]", "任务中心 · 今日 [F1]"),', 'tr(app, "Mission Control", "任务中心"),')
replace('src/ui.rs', '            LayoutMode::Compact => format!(\n                "{prefix}{pin}{collision}{locality} {} {} {} {}",', '''            LayoutMode::Compact if area.width < 60 => format!(
                "{prefix}{pin}{collision}{locality} {} {}",
                fit_display(if attention == "-" { runtime_status_label(&thread.runtime, app.language) } else { attention }, 9),
                sanitize_inline(thread.display_title())
            ),
            LayoutMode::Compact => format!(
                "{prefix}{pin}{collision}{locality} {} {} {} {}",''')
replace('src/ui/mission.rs', '    if width < 100 {', '''    if width < 60 {
        tr(app, "F1 Today F2 Projects F3 Review / ?", "F1 今日 F2 项目 F3 交付 /搜索 ?")
    } else if width < 100 {''')
replace('src/ui/mission.rs', 'Enter open  Ctrl+K / ?', 'Enter open  Ctrl+K / search ?')
replace('src/ui/mission.rs', 'Enter 打开  Ctrl+K / ?', 'Enter 打开  Ctrl+K / 搜索 ?')
replace('src/ui/mission.rs', 'o delivery  Ctrl+K actions', 'o delivery  / search  Ctrl+K actions')
replace('src/ui/mission.rs', 'o 交付  Ctrl+K 操作', 'o 交付  / 搜索  Ctrl+K 操作')
# Complete help stays aligned with executable key bindings; footer shows only daily routes.
replace('src/ui.rs', 'const HELP_LINES: &[&str] = &[', 'const HELP_LINES: &[&str] = &[\n    "Daily: F1 Today · F2 Projects · F3 Review queue",')
replace('src/ui.rs', 'const HELP_LINES_ZH: &[&str] = &[', 'const HELP_LINES_ZH: &[&str] = &[\n    "日常: F1 今日 · F2 项目 · F3 交付评审",')
replace('src/ui.rs', '· e alias · x ack",', '· e alias · x ack · o delivery",')
replace('src/ui.rs', 'Board: Tab Saved View · . batch-local/context · Enter open · a Quick Prompt · n Scratch', 'Board: Tab Saved View · . batch-local/context · Enter open · a Quick Prompt · n Scratch · o delivery')
replace('src/ui.rs', '· e 别名 · x 已处理",', '· e 别名 · x 已处理 · o 交付",')
replace('src/ui.rs', '看板: Tab 已保存视图 · . 本地批量/上下文 · Enter 打开 · a Quick Prompt · n Scratch', '看板: Tab 已保存视图 · . 本地批量/上下文 · Enter 打开 · a Quick Prompt · n Scratch · o 交付')
p=Path('src/ui.rs'); s=p.read_text()
for line, marker in [('    "Authority: Codex/Git/Forge stay canonical; codex-tui stores operator state only.",\n', 'const HELP_LINES: &[&str] = &[\n'), ('    "权限边界: Codex/Git/Forge 保持权威来源；codex-tui 只保存操作员状态。",\n', 'const HELP_LINES_ZH: &[&str] = &[\n')]:
    assert s.count(line)==1
    s=s.replace(line,'').replace(marker,marker+line,1)
p.write_text(s)
# Membership and per-thread-state tests explicitly choose identities, not obsolete visual offsets.
p=Path('src/app/tests.rs'); s=p.read_text()
a=s.index('fn registry_repo_only_filter_keeps_unresolved_and_degraded_candidates()'); b=s.index('\n#[test]',a)
part=s[a:b]; anchor='    let mut app = app();'; assert anchor in part
part=part.replace(anchor,anchor+'\n    app.show_all_history = true; // Membership is independent of Today ordering.',1)
s=s[:a]+part+s[b:]
a=s.index('fn per_thread_draft_scroll_and_follow_are_independent()'); b=s.index('\n#[test]',a)
part=s[a:b]
part=part.replace('    reduce(&mut app, Action::OpenSelected);','    app.selected = app.threads.iter().position(|thread| thread.id.0 == "thread-impl").unwrap();\n    reduce(&mut app, Action::OpenSelected);',1)
part=part.replace('    reduce(&mut app, Action::MoveSelection(1));','    app.selected = app.threads.iter().position(|thread| thread.id.0 == "thread-kws").unwrap();')
s=s[:a]+part+s[b:]; p.write_text(s)
p=Path('src/ui/tests.rs'); s=p.read_text()
s=s.replace('assert!(snapshot.contains("l local-only"));','assert!(HELP_LINES.iter().any(|line| line.contains("l local-only")));')
s=s.replace('assert!(snapshot.contains("g repo-only"));','assert!(HELP_LINES.iter().any(|line| line.contains("g repo-only")));')
old='''fn registry_footer_exposes_terminal_shortcut() {
    let snapshot = render_snapshot(100);
    assert!(
        snapshot.contains("t terminal"),
        "Mission Control must expose the Terminal Drawer shortcut"
    );
}'''
new='''fn registry_footer_exposes_daily_routes_and_help_keeps_advanced_terminal_shortcut() {
    let snapshot = render_snapshot(100);
    for route in ["F1 Today", "F2 Projects", "F3 Review", "Ctrl+K"] {
        assert!(snapshot.contains(route), "missing daily route {route}");
    }
    assert!(HELP_LINES.iter().any(|line| line.starts_with("Terminal drawer:") && line.contains("t open")));
}'''
assert old in s; s=s.replace(old,new); p.write_text(s)
