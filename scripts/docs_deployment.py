#!/usr/bin/env python3
"""Select documentation publication and verify the served same-source manifest."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import urllib.parse
import urllib.request
import check_docs_publication
import validation


def needed(root, event, env):
    if env.get('GITHUB_REF')!='refs/heads/main':raise ValueError('Documentation deployment requires main')
    if env['GITHUB_EVENT_NAME']=='workflow_dispatch':return True
    if env['GITHUB_EVENT_NAME']!='push':raise ValueError('Unsupported documentation deployment event')
    try:
        paths,_,_=validation.changes(root,'range',event['before'],env['GITHUB_SHA'])
    except validation.ValidationError:
        return True  # Unknown inputs require rebuilding trusted documentation.
    manifest=json.loads((root/'docs/translations.json').read_text())
    inputs={entry[field] for entry in manifest['documents'] for field in ('source','translation')}
    inputs.update({'docs/translations.json','LICENSE','.gitattributes',
                   '.github/workflows/docs-deploy.yml','scripts/docs_deployment.py',
                   'scripts/check_docs.py','scripts/check_docs_publication.py',
                   'scripts/check_public_boundary.py','scripts/validation.py'})
    return any(path in inputs or path.startswith('docs-site/') for path in paths)


def verify_bytes(raw, commit, digest):
    if not re.fullmatch('[a-f0-9]{64}',digest) or hashlib.sha256(raw).hexdigest()!=digest:
        raise ValueError('Served documentation manifest differs from the built artifact')
    check_docs_publication.validate_manifest(json.loads(raw),commit)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self,*args,**kwargs):return None


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('command',choices=['plan','manifest','verify']);parser.add_argument('--url');parser.add_argument('--sha256')
    args=parser.parse_args();root=validation.ROOT
    try:
        if args.command=='plan':
            event=json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
            with open(os.environ['GITHUB_OUTPUT'],'a') as stream:stream.write('needed='+str(needed(root,event,os.environ)).lower()+'\n')
        elif args.command=='manifest':
            raw=(root/'.local/docs-site/dist/build-manifest.json').read_bytes()
            check_docs_publication.validate_manifest(json.loads(raw),os.environ['GITHUB_SHA'])
            with open(os.environ['GITHUB_OUTPUT'],'a') as stream:stream.write('sha256='+hashlib.sha256(raw).hexdigest()+'\n')
        else:
            url=urllib.parse.urlsplit(args.url)
            if url.scheme!='https' or not url.hostname or url.username or url.password or url.query or url.fragment:raise ValueError('Invalid Pages deployment URL')
            request=urllib.request.Request(args.url.rstrip('/')+'/build-manifest.json',headers={'Cache-Control':'no-cache'})
            with urllib.request.build_opener(urllib.request.ProxyHandler({}),NoRedirect()).open(request,timeout=30) as response:
                raw=response.read(2*1024*1024+1)
            if len(raw)>2*1024*1024:raise ValueError('Served manifest exceeds bound')
            verify_bytes(raw,os.environ['GITHUB_SHA'],args.sha256)
            print('Served documentation manifest matches the verified source artifact')
    except (OSError,ValueError,TypeError,KeyError,validation.ValidationError):
        raise SystemExit('Documentation publication selection or verification failed')


if __name__=='__main__':main()
