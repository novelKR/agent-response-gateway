#!/usr/bin/env python3
"""Build committed Web source once and verify immutable assets for package consumers."""
import argparse
import os
from pathlib import Path
import shutil
import sys
import tarfile
import tempfile

import check_public_boundary
import release_package as package

ROOT=Path(__file__).resolve().parents[1]


def verify(source, directory, commit):
    package.require(directory.is_dir() and not directory.is_symlink(),'Web assets are not a regular directory')
    files={}
    for path in directory.rglob('*'):
        package.require(not path.is_symlink(),'Web assets contain a link')
        if path.is_dir():continue
        package.require(path.is_file(),'Web assets contain a special file')
        name=path.relative_to(directory).as_posix();package.relative_name(name)
        files[name]=package.read(path)
    manifest=package.json_value(files['web-manifest.json'])
    package.require(manifest['schema']=='gateway-management-web/v1' and manifest['api_contract']=='gateway-management-http/v1'
                    and manifest['state_contract']=='gateway-management-state/v1' and manifest['read_only'] is True
                    and manifest['source_commit']==commit and manifest['source_dirty'] is False,'Web source or contract differs')
    package.require(set(files)==set(manifest['files'])|{'web-manifest.json'},'Web asset inventory differs')
    package.require(all(package.sha(files[n])==h for n,h in manifest['files'].items()),'Web asset digest differs')
    package.require({'index.html','LICENSE.txt','web-notices.txt','web-dependencies.json'}<=set(files),'Web required asset missing')
    package.require(files['LICENSE.txt']==package.read(source/'LICENSE') and files['web-dependencies.json']==package.read(source/'management-web/licensing/dependencies.json'),'Web reviewed notices differ')
    return files


def npm_command(env):
    path=shutil.which('npm.cmd' if os.name=='nt' else 'npm',path=env['PATH'])
    package.require(path is not None,'Pinned npm is missing')
    path=Path(path)
    script=path.parent/'node_modules/npm/bin/npm-cli.js' if os.name=='nt' else path.resolve()
    package.require(script.is_file(),'Pinned npm CLI is missing')
    return ['node',str(script)]


def build(root, output):
    package.require(not output.exists(),'Web output already exists')
    commit=package.run(['git','rev-parse','HEAD'],root).decode().strip()
    state=root/'.local/web-build';state.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(dir=state) as temporary:
        temp=Path(temporary);archive=temp/'source.tar'
        archive.write_bytes(package.run(['git','archive','--format=tar','--prefix=source/',commit],root))
        check_public_boundary.Checker(root,[]).archive(archive)
        with tarfile.open(archive) as tar:
            files={m.name.removeprefix('source/'):package.sha(tar.extractfile(m).read()) for m in tar.getmembers() if m.isfile()}
            tar.extractall(temp,filter='data')
        source=temp/'source';receipt=temp/'source-receipt.json'
        receipt.write_bytes(package.encoded(dict(schema='gateway-source-export/v1',source_commit=commit,files=files)))
        config=temp/'npmrc';config.write_text('')
        env={k:v for k,v in os.environ.items() if k in {'PATH','HOME','SystemRoot','SYSTEMROOT','WINDIR','TEMP','TMP'}}
        env['NPM_CONFIG_USERCONFIG']=str(config)
        npm=npm_command(env)
        package.require(package.run(npm+['--version'],source,env).decode().strip()=='11.19.0','Pinned npm is required')
        package.run(npm+['ci','--prefix','management-web','--ignore-scripts'],source,env)
        package.run(npm+['run','build','--prefix','management-web','--','--source-receipt',str(receipt)],source,env)
        package.run(['node','management-web/scripts/check-output.mjs',commit],source,env)
        assets=verify(source,source/'.local/management-web/dist',commit)
        for name,raw in assets.items():package.write_new(output/name,raw)
    print('Verified exported Web source: '+commit)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('output',type=Path)
    try:build(ROOT,parser.parse_args().output)
    except (OSError,ValueError,KeyError,TypeError,package.PackageError,check_public_boundary.BoundaryError):
        raise SystemExit('Web source build or artifact verification failed')
