"""One-shot alias target correction, removed before final native CI."""
from pathlib import Path
p=Path('src/app.rs'); text=p.read_text()
def replace(old,new):
    global text
    if text.count(old)!=1: raise SystemExit('alias baseline changed: '+old[:80])
    text=text.replace(old,new,1)
replace('    input_original: String,\n','    input_original: String,\n    alias_target: Option<ThreadId>,\n')
replace('            input_original: String::new(),\n','            input_original: String::new(),\n            alias_target: None,\n')
replace('''        Action::BeginAlias => {
            if let Some(alias) = state
                .selected_thread()
                .map(|thread| thread.alias.clone().unwrap_or_default())
            {
                state.input_original.clear();''','''        Action::BeginAlias => {
            if let Some((target, alias)) = state.selected_thread().map(|thread| {
                (thread.id.clone(), thread.alias.clone().unwrap_or_default())
            }) {
                state.alias_target = Some(target);
                state.input_original.clear();''')
replace('''            if mode == InputMode::Alias {
                let alias = state.input_buffer.trim().to_string();
                if let Some(thread) = state.threads.get_mut(state.selected) {''','''            if mode == InputMode::Alias {
                let Some(index) = state.alias_target.as_ref()
                    .and_then(|id| state.thread_index_by_id.get(&id.0)).copied()
                else {
                    state.mutation_notice = Some(local_text(state.language,
                        "alias target is no longer available; edit retained, no changes saved",
                        "别名目标已不可用；已保留输入，未保存任何更改").into());
                    return vec![];
                };
                let alias = state.input_buffer.trim().to_string();
                if let Some(thread) = state.threads.get_mut(index) {''')
# Restrict completion edit to the alias block, not unrelated editors.
start=text.index('            if mode == InputMode::Alias {')
end=text.index('            let was_search = mode == InputMode::Search;',start)
block=text[start:end]
marker='                    state.input_mode = InputMode::Normal;'
if block.count(marker)!=1: raise SystemExit('alias completion changed')
text=text[:start]+block.replace(marker,'                    state.alias_target = None;\n'+marker,1)+text[end:]
replace('''        Action::CancelInput => {
            let mut search_watch_to_release = None;''','''        Action::CancelInput => {
            state.alias_target = None;
            let mut search_watch_to_release = None;''')
p.write_text(text)
