"""Explicit activation of the locally built reference codec for synthetic tests."""
import importlib.util
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('codec_extension_manager',ROOT/'scripts/extension_manager.py')
manager=importlib.util.module_from_spec(spec);spec.loader.exec_module(manager)

def activate(executable,folder,raw,store=None):
    folder=Path(folder).resolve();folder.mkdir(mode=0o700,parents=True,exist_ok=False)
    if store is None:store=folder/'store'
    package=folder/'package'
    digest=manager.package_binary(Path(executable).resolve(),ROOT/'LICENSE',package,'reference-codec','1.0.0','api_codec')
    manager.install(store,package,digest)
    manager.enable(store,'reference-codec','1.0.0',digest,manager.CODEC_PERMISSIONS)
    lines=[]
    for line in raw.splitlines():
        lines.append(line)
        if line.startswith('[models.') and line.endswith(']'):
            lines.append('api_codec="reference-codec"')
    return '\n'.join(lines)+'\n',store/'active.json'
