"""Command schema for the offline mock core.

Parses the `DeviceCommand` enum in src/device.rs (the exact wire contract serde enforces) so the mock
`send_command` can reject what the real core would reject: unknown actions, missing required fields and
unknown field names. Handles `rename`, `alias`, `default`, `flatten`, `skip_deserializing`, container
`rename_all` and `Option<T>` (optional without `default`). Only top-level fields are checked; nested
payloads are the Rust types' own business.
"""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
DEVICE_RS = ROOT / 'src' / 'device.rs'


def camel(name):
    return name[:1].lower() + name[1:]


def snake_to_camel(name):
    head, *rest = name.split('_')
    return head + ''.join(p[:1].upper() + p[1:] for p in rest)


def _serde_items(attr):
    """Split the inside of #[serde(...)] on top-level commas."""
    items, depth, cur = [], 0, ''
    for ch in attr:
        if ch in '([':
            depth += 1
        elif ch in ')]':
            depth -= 1
        if ch == ',' and depth == 0:
            items.append(cur.strip())
            cur = ''
        else:
            cur += ch
    if cur.strip():
        items.append(cur.strip())
    return items


def _parse_attrs(attrs):
    out = {'rename': None, 'aliases': [], 'default': False, 'flatten': False, 'skip': False, 'rename_all': None}
    for attr in attrs:
        for item in _serde_items(attr):
            m = re.match(r'(\w+)\s*=\s*"([^"]*)"', item)
            if m:
                key, value = m.groups()
                if key == 'rename':
                    out['rename'] = value
                elif key == 'alias':
                    out['aliases'].append(value)
                elif key == 'default':
                    out['default'] = True
                elif key == 'rename_all':
                    out['rename_all'] = value
            elif item == 'default':
                out['default'] = True
            elif item == 'flatten':
                out['flatten'] = True
            elif item in ('skip', 'skip_deserializing'):
                out['skip'] = True
    return out


def _block(source, header_regex):
    """Return (container attributes, body text) of the first item whose header matches."""
    m = re.search(header_regex, source)
    if not m:
        return None, None
    start = source.index('{', m.end() - 1)
    depth, i = 0, start
    while i < len(source):
        if source[i] == '{':
            depth += 1
        elif source[i] == '}':
            depth -= 1
            if depth == 0:
                break
        i += 1
    head = source[:m.start()].rstrip().split('\n')
    attrs = []
    for line in reversed(head):
        t = line.strip()
        if t.startswith('#['):
            attrs.append(t)
        elif t.startswith('///') or not t:
            continue
        else:
            break
    return attrs, source[start + 1:i]


def _items(body, indent):
    """Yield (name, attrs, header_line, end_offset) for each item at exactly `indent` (a variant or a field).
    Nested content sits at a deeper indent, so it is skipped without tracking braces."""
    pending, collecting, offset = [], None, 0
    for raw in body.split(chr(10)):
        line = raw.rstrip()
        start = offset
        offset += len(raw) + 1
        t = line.strip()
        if collecting is not None:
            collecting += ' ' + t
            if t.endswith(')]'):
                pending.append(collecting)
                collecting = None
            continue
        lead = len(line) - len(line.lstrip())
        if lead != indent or not t:
            continue
        if t.startswith('#[serde('):
            if t.endswith(')]'):
                pending.append(t)
            else:
                collecting = t
            continue
        if t.startswith('#[') or t.startswith('//'):
            continue
        m = re.match(r'^(?:pub\s+)?(\w+)\s*(\{|,|\(|:)', t)
        if m:
            yield m.group(1), [re.sub(r'^#\[serde\((.*)\)\]$', r'\1', a) for a in pending], t, start + (line.rindex('{') if line.endswith('{') else 0)
            pending = []


def _fields(body, indent, container_rename_all, container_default=False):
    fields, flatten = {}, []
    for name, attrs, header, _ in _items(body, indent):
        a = _parse_attrs(attrs)
        if a['skip']:
            continue
        type_text = header.split(':', 1)[1].strip().rstrip(',') if ':' in header else ''
        if a['flatten']:
            flatten.append(type_text.split('::')[-1].split('<')[0].strip())
            continue
        wire = a['rename'] or (snake_to_camel(name) if container_rename_all == 'camelCase' else name)
        required = not (a['default'] or container_default or type_text.startswith('Option<'))
        fields[wire] = {'required': required, 'aliases': a['aliases']}
    return fields, flatten


def _struct_fields(name, sources, depth=0):
    for source in sources:
        attrs, body = _block(source, r'(?:pub\s+)?struct\s+' + re.escape(name) + r'\s*\{')
        if body is None:
            continue
        rename_all, container_default = None, False
        for a in attrs:
            inner = re.sub(r'^#\[serde\((.*)\)\]$', r'\1', a)
            parsed = _parse_attrs([inner])
            rename_all = parsed['rename_all'] or rename_all
            container_default = container_default or parsed['default']
        fields, flatten = _fields(body, 4, rename_all, container_default)
        unknown_ok = False
        for inner in flatten:
            if depth > 3:
                unknown_ok = True
                continue
            sub = _struct_fields(inner, sources, depth + 1)
            if sub is None:
                unknown_ok = True
            else:
                fields.update(sub['fields'])
                unknown_ok = unknown_ok or sub['open']
        return {'fields': fields, 'open': unknown_ok}
    return None


def parse(source, extra_sources=()):
    """-> {action: {'fields': {wire: {required, aliases}}, 'open': bool}}"""
    attrs, body = _block(source, r'enum\s+DeviceCommand\s*\{')
    if body is None:
        raise ValueError('DeviceCommand enum not found')
    sources = [source, *extra_sources]
    schema = {}
    for name, vattrs, header, end in _items(body, 4):
        v = _parse_attrs(vattrs)
        action = v['rename'] or camel(name)
        if header.endswith('{'):
            sub = body[end:]
            depth, i = 0, 0
            for i, ch in enumerate(sub):
                if ch == '{':
                    depth += 1
                elif ch == '}':
                    depth -= 1
                    if depth == 0:
                        break
            fields, flatten = _fields(sub[1:i], 8, None)
            is_open = False
            for inner in flatten:
                nested = _struct_fields(inner, sources)
                if nested is None:
                    is_open = True
                else:
                    fields.update(nested['fields'])
                    is_open = is_open or nested['open']
        else:
            fields, is_open = {}, False
        entry = {'fields': fields, 'open': is_open}
        schema[action] = entry
        for alias in v['aliases']:
            schema[alias] = entry
    return schema


def load(path=DEVICE_RS):
    sources = [Path(path).read_text(encoding='utf-8')]
    for extra in list((ROOT / 'src').rglob('*.rs')) + [ROOT / 'protocol' / 'src' / 'lib.rs']:
        if extra.exists() and extra.resolve() != Path(path).resolve():
            sources.append(extra.read_text(encoding='utf-8'))
    return parse(sources[0], sources[1:])


def validate(schema, command, ignore=('persistent',)):
    """Problems serde would report for this `send_command` payload (empty list = accepted)."""
    if not isinstance(command, dict):
        return ['command must be an object']
    action = command.get('action')
    if not isinstance(action, str) or not action:
        return ['missing field `action`']
    spec = schema.get(action)
    if spec is None:
        return [f'unknown action `{action}`']
    problems = []
    known = set()
    for wire, info in spec['fields'].items():
        known.add(wire)
        known.update(info['aliases'])
    keys = set(command) - {'action'} - set(ignore)
    for wire, info in spec['fields'].items():
        if info['required'] and wire not in command and not any(a in command for a in info['aliases']):
            problems.append(f'{action}: missing field `{wire}`')
    if not spec['open']:
        for key in sorted(keys - known):
            problems.append(f'{action}: unknown field `{key}`')
    return problems


if __name__ == '__main__':
    parsed = load()
    print(f'{len(parsed)} actions')
