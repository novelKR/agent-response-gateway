"""Convert synthetic host declarations to an explicitly activated data pack.

Used by conformance fixtures only; never discovers or qualifies a real provider.
"""
import json
from pathlib import Path
import re
import subprocess
import tomllib


def activate(binary, folder, raw):
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=False)
    config = tomllib.loads(raw)
    capabilities = {}
    imports = []
    # Exports use independent portable identifiers, preserving host aliases verbatim.
    for index, (name, value) in enumerate(config.get('capability_profiles', {}).items()):
        value = dict(value)
        provider, model = value.pop('provider'), value.pop('upstream_model')
        value.pop('version')
        export = f'profile-{index}'
        capabilities[export] = value
        imports.append(f'[capability_profile_imports.{json.dumps(name)}]\npack="synthetic-fixture"\nexport={json.dumps(export)}\nprovider={json.dumps(provider)}\nupstream_model={json.dumps(model)}\n')
    policies = {}
    for index, (name, value) in enumerate(config.get('compatibility_policies', {}).items()):
        export = f'policy-{index}'
        policies[export] = value
        imports.append(f'[compatibility_policy_imports.{json.dumps(name)}]\npack="synthetic-fixture"\nexport={json.dumps(export)}\n')
    source = {'schema': 'gateway-profile-pack/v1', 'id': 'synthetic-fixture', 'version': '1.0.0',
              'capabilities': capabilities, 'policies': policies, 'evidence': [],
              'notices': {'LICENSE': 'Synthetic fixture declarations; no third-party source content.'}}
    source_path, package, store = folder / 'source.json', folder / 'pack.json', folder / 'store'
    source_path.write_text(json.dumps(source), encoding='utf-8')

    def run(*args):
        result = subprocess.run([str(binary), 'profile-pack', *map(str, args)], capture_output=True, check=True, timeout=10)
        return json.loads(result.stdout)

    report = run('package', '--source', source_path, '--output', package)
    run('install', '--package', package, '--store', store)
    run('enable', '--store', store, '--id', source['id'], '--version', source['version'], '--sha256', report['package_sha256'])
    lines, skip = [], False
    for line in raw.splitlines():
        if line.lstrip().startswith('['):
            skip = bool(re.match(r'\[(capability_profiles|compatibility_policies)(\.|\])', line.lstrip()))
        if not skip:
            lines.append(line)
    return '\n'.join(lines) + '\n' + '\n'.join(imports), store / 'active.json'
