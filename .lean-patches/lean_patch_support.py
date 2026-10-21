from pathlib import Path

def replace(path, old, new, count=1):
    p = Path(path)
    value = p.read_text(encoding='utf-8')
    actual = value.count(old)
    if actual != count:
        raise SystemExit('anchor count mismatch in %s: expected %s, got %s: %r' % (path, count, actual, old[:100]))
    p.write_text(value.replace(old, new), encoding='utf-8')

def between(path, start, end, value):
    p = Path(path)
    text = p.read_text(encoding='utf-8')
    if text.count(start) != 1:
        raise SystemExit('start anchor is not unique: ' + path)
    a = text.index(start)
    b = text.index(end, a + len(start))
    p.write_text(text[:a] + value + text[b:], encoding='utf-8')

def write(path, text):
    p = Path(path)
    if p.exists():
        raise SystemExit('refusing to replace existing new file: ' + path)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding='utf-8')
