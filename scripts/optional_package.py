#!/usr/bin/env python3
"""Build optional management, static Web and Team archives from a verified base source export."""
import argparse
from pathlib import Path
import re
import shutil
import sys
import tarfile
import tempfile

import check_public_boundary
import web_assets
import release_package as package
from release_targets import TARGETS

SCHEMA = 'gateway-optional-candidate/v1'
MODULE = 'gateway-optional-module/v1'
CONTRACTS = {'management':'gateway-management-http/v1','web':'gateway-management-web/v1','team':'gateway-team-http/v1'}
require = package.require
read, sha, encoded = package.read, package.sha, package.encoded
ROOT = Path(__file__).resolve().parents[1]


def extract(files, destination):
    for name, (raw, mode) in files.items():
        package.relative_name(name)
        file = destination / name
        package.write_new(file, raw)
        file.chmod(mode)


def module(kind, commit, target, files):
    return {'schema':MODULE,'module':kind,'contract':CONTRACTS[kind], 'source_commit':commit,
            'target':None if kind=='web' else target, 'requires':{} if kind=='management' else {'management':commit},
            'native_extensions':kind=='management' and not target.endswith('windows-msvc'),
            'native_recorder':kind in {'management','team'} and not target.endswith('windows-msvc'),
            'python_requirement':'3.11+ for native extension management' if kind=='management' else None,
            'runtime_node_required':False,'enabled_automatically':False,
            'files':{name:{'sha256':sha(raw),'mode':mode} for name,(raw,mode) in sorted(files.items())}}


def verify_module(kind, commit, target, files):
    path = f'gateway-management/modules/{kind}.json'
    require(path in files and files[path][1]==0o644,'missing module manifest')
    value=package.json_value(files[path][0]);payload={k:v for k,v in files.items() if k!=path}
    require(value==module(kind,commit,target,payload),'module identity, compatibility or file binding differs')
    checker=check_public_boundary.Checker(ROOT,[])
    for name,(raw,mode) in files.items():
        checker.path(name.encode(),f'100{mode:o}'.encode());checker.size(len(raw));checker.content(raw)
    if kind=='web':
        prefix='gateway-management/web/'
        manifest=package.json_value(files[prefix+'web-manifest.json'][0])
        require(manifest['schema']==CONTRACTS['web'] and manifest['api_contract']==CONTRACTS['management']
                and manifest['state_contract']=='gateway-management-state/v1' and manifest['read_only'] is True
                and manifest['source_commit']==commit and manifest['source_dirty'] is False,'Web source or contract differs')
        require(set(name.removeprefix(prefix) for name in files if name.startswith(prefix))=={*manifest['files'],'web-manifest.json'},'Web archive inventory differs')
        require(all(sha(files[prefix+name][0])==digest for name,digest in manifest['files'].items()),'Web asset hash differs')
    else:
        names=['gateway-team-manager'] if kind=='team' else ['gateway-manager','gateway-management-cli','gateway-managed-child']
        for name in names:
            executable=f'gateway-management/bin/{name}'+('.exe' if target.endswith('windows-msvc') else '')
            require(executable in files and files[executable][1]==0o755,'required optional executable missing')
        for name in ['LICENSE','license-notices/THIRD-PARTY-NOTICES.md'] + (['scripts/extension_manager.py'] if kind=='management' else []):
            require('gateway-management/'+name in files,'required original notices missing')
    return value


def verify(directory, commit=None, target=None):
    value=package.json_value(read(directory/'optional-candidate.json'))
    require(value['schema']==SCHEMA and value['target'] in TARGETS and re.fullmatch(r'[a-f0-9]{40}',value['source_commit']),'invalid optional candidate identity')
    require(commit in {None,value['source_commit']} and target in {None,value['target']},'optional candidate binding differs')
    require(set(value['modules'])==set(CONTRACTS),'incomplete optional modules')
    require(set(p.name for p in directory.iterdir())=={*value['assets'],'optional-candidate.json','SHA256SUMS'},'unexpected candidate file')
    for name,digest in value['assets'].items():
        require(package.relative_name(name).name==name and sha(read(directory/name))==digest,'optional asset hash differs')
    sums={**value['assets'],'optional-candidate.json':sha(read(directory/'optional-candidate.json'))}
    require(read(directory/'SHA256SUMS')==''.join(f'{v}  {k}\n' for k,v in sorted(sums.items())).encode(),'optional checksums differ')
    source=directory/value['source_archive'];check_public_boundary.Checker(ROOT,[]).archive(source)
    with tarfile.open(source,tarinfo=check_public_boundary.BoundedTarInfo) as archive:
        require(archive.pax_headers.get('comment')==value['source_commit'],'optional source receipt differs')
        for name,digest in value['packaging_tools'].items():
            member=archive.getmember('agent-response-gateway/scripts/'+name)
            require(member.isfile() and sha(archive.extractfile(member).read())==digest,'optional packaging source differs')
        require(sha(archive.extractfile('agent-response-gateway/Cargo.lock').read())==value['cargo_lock_sha256'],'optional lock binding differs')
    for kind,name in value['modules'].items():
        verify_module(kind,value['source_commit'],value['target'],package.archive_files(directory/name))
    return value


def inventory(metadata, built):
    # Raw Cargo path IDs are local evidence only and must never enter a public archive.
    result=[]
    for entry in metadata['packages']:
        if entry['id'] in built:
            result.append({'name':entry['name'],'version':entry['version'],'origin':'workspace' if entry['source'] is None else entry['source'],
                           'features':sorted(built[entry['id']]['features'])})
    return sorted(result,key=lambda v:(v['name'],v['version']))


def build(root, base_directory, output, web_directory=None):
    base=package.verify_candidate(base_directory)
    target,commit=base['target'],base['source_commit']
    require(package.run(['git','rev-parse','HEAD'],root).decode().strip()==commit,'base source does not match the selected checkout')
    require(not output.exists(),'optional destination must not exist')
    parent=root/'.local/optional-build';parent.mkdir(parents=True,exist_ok=True)
    logs=Path(tempfile.mkdtemp(prefix='logs-',dir=parent))
    with tempfile.TemporaryDirectory(prefix='source-',dir=parent) as temporary:
        temporary=Path(temporary);source_archive=base_directory/base['source_archive']
        with tarfile.open(source_archive) as archive:
            members=archive.getmembers();source_files={m.name.removeprefix('agent-response-gateway/'):sha(archive.extractfile(m).read()) for m in members if m.isfile()}
            archive.extractall(temporary,filter='data')
        source=temporary/'agent-response-gateway'
        for name in ['optional_package.py','management_smoke.py','web_assets.py']:
            require(read(root/'scripts'/name)==read(source/'scripts'/name),'packaging recipe is not the committed source')
        receipt=temporary/'source-receipt.json';package.write_new(receipt,encoded({'schema':'gateway-source-export/v1','source_commit':commit,'files':source_files}))
        env,sysroot=package.clean_environment(root/'target/release-candidate'/target,source)
        suffix='.exe' if target.endswith('windows-msvc') else ''
        release=Path(env['CARGO_TARGET_DIR'])/target/'release'
        common={name.replace('agent-response-gateway/','gateway-management/',1):value for name,value in package.archive_files(base_directory/base['binary_archive']).items()
                if name.startswith(('agent-response-gateway/license-notices/','agent-response-gateway/rust-notices/')) or name=='agent-response-gateway/LICENSE'}
        for name in ['docs/standalone-management.md','docs/ko/standalone-management.md','scripts/extension_manager.py']:
            common['gateway-management/'+name]=(read(source/name),0o644)
        metadata=package.json_value(package.run(['cargo','metadata','--format-version=1','--locked','--offline','--filter-platform',target],source,env))
        command=['cargo','build','--release','--locked','--offline','--target',target,'--message-format=json']
        print('optional-package: build management with Team feature disabled',flush=True)
        raw=package.run(command+['-p','gateway-management-app','--bin','gateway-manager','-p','gateway-management-runtime','--bin','gateway-managed-child','-p','gateway-management-api','--bin','gateway-management-cli'],source,env,logs/'management-cargo.log')
        management=dict(common)
        for name in ['gateway-manager','gateway-managed-child','gateway-management-cli']:
            management['gateway-management/bin/'+name+suffix]=(read(release/(name+suffix)),0o755)
        management_inventory=inventory(metadata,package.compiled_packages(raw))
        require(not any(p['name'].startswith('gateway-team-') for p in management_inventory),'Team dependency entered the management-only build')
        if not suffix:
            package.run(command+['-p','gateway-usage-recorder','--bin','gateway-usage-recorder'],source,env,logs/'recorder-cargo.log')
            management['gateway-management/bin/gateway-usage-recorder']=(read(release/'gateway-usage-recorder'),0o755)
        print('optional-package: build explicitly selected Team manager',flush=True)
        raw=package.run(command+['-p','gateway-management-app','--features','team','--bin','gateway-team-manager'],source,env,logs/'team-cargo.log')
        team=dict(common);team['gateway-management/bin/gateway-team-manager'+suffix]=(read(release/('gateway-team-manager'+suffix)),0o755)
        team_inventory=inventory(metadata,package.compiled_packages(raw))
        if web_directory is None:
            print('optional-package: verify and build static Web from exact exported source',flush=True)
            empty_npm=temporary/'npmrc';package.write_new(empty_npm,b'');node_env={**env,'NPM_CONFIG_USERCONFIG':str(empty_npm)}
            npm_path=shutil.which('npm.cmd' if suffix else 'npm',path=node_env['PATH'])
            require(npm_path is not None,'pinned npm executable is missing')
            # Invoke the installed npm CLI with Node directly: Windows .cmd quoting and
            # implicit command-shell behavior are not part of the source-build recipe.
            npm_script=Path(npm_path).parent/'node_modules/npm/bin/npm-cli.js' if suffix else Path(npm_path).resolve()
            require(npm_script.is_file(),'installed npm CLI entry point is missing')
            npm=['node',npm_script]
            print('optional-package: verify pinned npm version',flush=True)
            require(package.run(npm+['--version'],source,node_env).decode().strip()=='11.19.0','pinned npm is required')
            print('optional-package: install locked Web dependencies',flush=True)
            package.run(npm+['ci','--prefix','management-web','--ignore-scripts'],source,node_env,logs/'web-install.log')
            print('optional-package: build Web against source receipt',flush=True)
            package.run(npm+['run','build','--prefix','management-web','--','--source-receipt',receipt],source,node_env,logs/'web-build.log')
            print('optional-package: verify Web output',flush=True)
            package.run(['node','management-web/scripts/check-output.mjs',commit],source,node_env,logs/'web-check.log')
            web_directory=source/'.local/management-web/dist'
        verified_web=web_assets.verify(source,web_directory,commit)
        web={'gateway-management/web/'+name:(raw,0o644) for name,raw in verified_web.items()}
        payloads={'management':management,'web':web,'team':team};output.mkdir(parents=True)
        names={};version=base['version']
        for kind,files in payloads.items():
            files[f'gateway-management/modules/{kind}.json']=(encoded(module(kind,commit,target,files)),0o644)
            name=f'gateway-{kind}-{version}'+('' if kind=='web' else '-'+target)+('.tar.gz' if kind=='web' else '.'+TARGETS[target]['archive'])
            names[kind]=name;package.write_new(output/name,package.tar_bytes(files) if kind=='web' else package.archive_bytes(files,target))
            verify_module(kind,commit,target,package.archive_files(output/name))
        merged={}
        for kind in CONTRACTS:
            for name,value in package.archive_files(output/names[kind]).items():
                require(name not in merged or merged[name]==value,'optional modules collide')
                merged[name]=value
        destination=temporary/'extracted optional package';extract(merged,destination)
        base_files=package.archive_files(base_directory/base['binary_archive']);extract(base_files,temporary/'base extracted')
        binary_root=destination/'gateway-management/bin';gateway=temporary/'base extracted/agent-response-gateway/bin'/TARGETS[target]['executable']
        print('optional-package: execute extracted management, Web and Team combinations',flush=True)
        combinations=[('management','gateway-manager',False,False),('web','gateway-manager',True,False),('team-disabled','gateway-team-manager',False,False),('team','gateway-team-manager',True,True)]
        for label,executable,with_web,with_team in combinations:
            args=[sys.executable,'-B',source/'scripts/management_smoke.py','--manager',binary_root/(executable+suffix),'--child',binary_root/('gateway-managed-child'+suffix),'--cli',binary_root/('gateway-management-cli'+suffix),'--gateway',gateway,'--state-dir',temporary/('smoke-'+label)]
            if with_web:args+=['--web',destination/'gateway-management/web']
            if with_team:args+=['--team','--managed']
            if with_web and not suffix:args+=['--recorder',binary_root/'gateway-usage-recorder','--python',Path(sys.executable).resolve(),'--extension-manager',destination/'gateway-management/scripts/extension_manager.py']
            package.run(args,source,env,logs/('smoke-'+label+'.log'),timeout=180)
        package.write_new(output/base['source_archive'],read(source_archive))
        value={'schema':SCHEMA,'source_commit':commit,'target':target,'version':version,'source_archive':base['source_archive'],
               'cargo_lock_sha256':sha(read(source/'Cargo.lock')),'modules':names,'module_contracts':CONTRACTS,
               'compilation':{'management':management_inventory,'team':team_inventory},
               'packaging_tools':{n:sha(read(source/'scripts'/n)) for n in ['optional_package.py','management_smoke.py','release_package.py','web_assets.py']},
               'build':{'profile':'release','locked':True,'offline_rust':True,'node':'24.21.0','npm':'11.19.0','rustc':base['rustc']},
               'validation':{'extracted_combinations':[c[0] for c in combinations],'provider_calls':'synthetic_only','deployment':'not_performed','consumer_acceptance':'not_performed'},
               'assets':{p.name:sha(read(p)) for p in output.iterdir()}}
        package.write_new(output/'optional-candidate.json',encoded(value))
        sums={p.name:sha(read(p)) for p in output.iterdir()};package.write_new(output/'SHA256SUMS',''.join(f'{v}  {k}\n' for k,v in sorted(sums.items())).encode())
        return verify(output,commit,target)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);commands=parser.add_subparsers(dest='command',required=True)
    create=commands.add_parser('build');create.add_argument('--base',required=True,type=Path);create.add_argument('--output',required=True,type=Path);create.add_argument('--web-assets',type=Path)
    check=commands.add_parser('verify');check.add_argument('directory',type=Path);check.add_argument('--commit');check.add_argument('--target')
    args=parser.parse_args()
    try:
        result=build(ROOT,args.base.resolve(),args.output.resolve(),args.web_assets.resolve() if args.web_assets else None) if args.command=='build' else verify(args.directory.resolve(),args.commit,args.target)
        print('optional-package: verified '+result['source_commit']+' '+result['target'])
    except (package.PackageError,check_public_boundary.BoundaryError,OSError,ValueError,KeyError,TypeError) as error:
        # PackageError messages are fixed diagnostics; arbitrary OS/input text stays private.
        detail=str(error) if isinstance(error,package.PackageError) else type(error).__name__
        parser.exit(1,'optional-package: '+detail+'; inspect ignored build logs\n')
