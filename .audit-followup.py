from pathlib import Path
if Path('.audit-followup-applied').exists():
    raise SystemExit(0)
p=Path('.')
f=p/'src/app_server/pagination.rs';s=f.read_text();s=s.replace('let mut budget = PageBudget::default();\n        budget.pages = MAX_PAGES;', 'let mut budget = PageBudget { pages: MAX_PAGES, ..PageBudget::default() };');f.write_text(s)
f=p/'src/app_server/tests.rs';s=f.read_text();s=s.replace('assert!(production.contains("registry_complete: true"));','assert!(include_str!("pagination.rs").split("#[cfg(test)]").next().unwrap().contains("registry_complete: true"));');f.write_text(s)
f=p/'src/app_server_transport/tests.rs';s=f.read_text();s=s.replace('// Poll consumes the prefix, then Pending cancels this exact read Future.','// Exclude cooperative-budget yields so cancellation happens after prefix consumption.\n        // Other tests exercise the production timer/command scheduling paths.');s=s.replace('read_stdio_json(&mut reader, &mut pending)\n                .now_or_never()', 'tokio::task::unconstrained(read_stdio_json(&mut reader, &mut pending))\n                .now_or_never()',1);f.write_text(s)
f=p/'scripts/testing/app_server_transport_regression.py';s=f.read_text();s=s.replace('import time\n','import time\nimport unicodedata\n',1) if 'import unicodedata' not in s else s
s=s.replace("def normalized(data):\n    return re.sub(rb'\\x1b\\[[0-?]*[ -/]*[@-~]', b'', data)",'''def screen_text(data):
    """Reconstruct this fixed Ratatui fixture, not a general terminal emulator.

    Raw ANSI-stripped output is not screen text: unchanged cells are not repainted.
    Unexpected cursor controls fail closed; an incomplete trailing CSI waits for bytes.
    """
    grid = [[' '] * 180 for _ in range(40)]
    row = col = offset = 0
    text = bytes(data).decode('utf-8', errors='replace')
    while offset < len(text):
        char = text[offset]
        if char == '\\x1b':
            match = re.match(r'\\x1b\\[([0-?]*)([ -/]*)([@-~])', text[offset:])
            if not match:
                require(text[offset:] == '\\x1b' or text[offset:].startswith('\\x1b['), 'unexpected non-CSI control')
                break
            arguments, intermediate, operation = match.groups()
            require(not intermediate, 'unexpected CSI intermediate')
            if operation in ('H', 'f'):
                values = (arguments or '1;1').split(';')
                row = int(values[0] or '1') - 1
                col = int(values[1] or '1') - 1 if len(values) > 1 else 0
                require(0 <= row < 40 and 0 <= col < 180, 'cursor outside fixture grid')
            elif operation == 'J':
                require(arguments in ('2', '3'), 'unsupported partial erase')
                grid = [[' '] * 180 for _ in range(40)]
            elif operation not in ('m', 'h', 'l'):
                raise RuntimeError('unexpected fixture CSI: ' + repr(match.group()))
            offset += len(match.group())
            continue
        if char == '\\r':
            col = 0
        elif char == '\\n':
            row = min(row + 1, 39)
        elif ord(char) >= 32 and not unicodedata.combining(char):
            if col < 180:
                grid[row][col] = char
                width = 2 if unicodedata.east_asian_width(char) in ('W', 'F') else 1
                if width == 2 and col + 1 < 180:
                    grid[row][col + 1] = ''
                col += width
        offset += 1
    return '\\n'.join(''.join(line) for line in grid)


def check_observer():
    sparse = b'\\x1b[1;1HAAA\\x1b[1;1HZ\\x1b[1;3HZ'
    require(screen_text(sparse).startswith('ZAZ'), 'observer lost an unchanged cell')
    require(screen_text(sparse + b'\\x1b[2;').startswith('ZAZ'), 'observer lost a fragmented CSI')
    require(screen_text(sparse + b'\\x1b[2;1HB').splitlines()[1].startswith('B'), 'observer cursor reconstruction failed')
    try:
        screen_text(b'\\x1b[2A')
    except RuntimeError:
        pass
    else:
        raise RuntimeError('observer accepted an unsupported cursor operation')''')
s=s.replace('screen_writes = normalized(raw)', 'screen_writes = screen_text(raw)')
s=s.replace("b'closed' in screen_writes and b'stdout' in screen_writes", "'closed' in screen_writes and 'stdout' in screen_writes")
s=s.replace("b'ZZZ_READ_OK_ZZZ' in screen_writes", "'ZZZ_READ_OK_ZZZ' in screen_writes")
s=s.replace("(output / (mode + '.terminal.bin')).write_bytes(raw)","(output / (mode + '.terminal.bin')).write_bytes(raw)\n            (output / (mode + '.screen.txt')).write_text(screen_text(raw), encoding='utf-8')")
s=s.replace('def main():\n', 'def main():\n    check_observer()\n') if '    check_observer()' not in s else s
f.write_text(s)
Path('.audit-followup-applied').write_text('observer-and-test-corrections-v1\n')
