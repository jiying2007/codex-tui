use std::io::{self, Write};

fn main() -> io::Result<()> {
    let markdown = codex_tui::syntax_highlight::asset_acknowledgements_markdown();
    let mut stdout = io::stdout().lock();
    stdout.write_all(markdown.as_bytes())?;
    if !markdown.ends_with('\n') {
        stdout.write_all(b"\n")?;
    }
    Ok(())
}
