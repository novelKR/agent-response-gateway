"""Full qualification and legacy evidence; all GitHub responses are synthetic."""
import base64
import copy
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
import check_ci_results
import release_qualification as q
import release_provenance as provenance
import test_release_provenance as legacy
import test_release_package as fixtures

POLICY=(ROOT/'scripts/validation-policy.json').read_bytes()
SUITES=(ROOT/'scripts/conformance-suites.json').read_bytes()


def evidence():
    return dict(schema=q.SCHEMA,source_commit=fixtures.COMMIT,policy_sha256=hashlib.sha256(POLICY).hexdigest(),run_id=123,attempt=1,targets=sorted(q.TARGETS),checks={n:'success' for n in check_ci_results.REQUIRED_JOBS})


class QualificationTests(unittest.TestCase):
    def test_complete_bound_evidence_passes(self):
        q.validate(evidence(),fixtures.COMMIT,POLICY,run_id=123,attempt=1)

    def test_missing_failed_skipped_and_rebound_evidence_fails(self):
        changes=[('source_commit','a'*40),('policy_sha256','0'*64),('run_id',456),('attempt',2),('targets',[]),('schema','unknown')]
        for key,value in changes:
            doc=evidence();doc[key]=value
            with self.subTest(key=key),self.assertRaises(ValueError):q.validate(doc,fixtures.COMMIT,POLICY,run_id=123,attempt=1)
        for status in ['failure','skipped','cancelled']:
            doc=evidence();doc['checks']['rust']=status
            with self.assertRaises(ValueError):q.validate(doc,fixtures.COMMIT,POLICY)
        doc=evidence();del doc['checks']['rust']
        with self.assertRaises(ValueError):q.validate(doc,fixtures.COMMIT,POLICY)

    def test_generic_main_success_cannot_be_recorded_as_qualification(self):
        policy=json.loads(POLICY);jobs={n:{'result':'success'} for n in check_ci_results.REQUIRED_JOBS}
        env=dict(GITHUB_SHA=fixtures.COMMIT,GITHUB_RUN_ID='123',GITHUB_RUN_ATTEMPT='1')
        plan=dict(schema='gateway-validation-plan/v1',source_sha=fixtures.COMMIT,base_sha=fixtures.COMMIT,head_sha=fixtures.COMMIT,
                  policy_sha256=hashlib.sha256(POLICY).hexdigest(),profile='full',stage='main',mode='shadow',jobs=policy['jobs'],execution_jobs=policy['jobs'])
        with self.assertRaises(ValueError):q.record(plan,jobs,env)
        plan['stage']='release';self.assertEqual(q.record(plan,jobs,env),evidence())
        plan['profile']='affected'
        with self.assertRaises(ValueError):q.record(plan,jobs,env)

    def test_live_job_evidence_cannot_be_replaced_by_workflow_success(self):
        class Github:
            def api(self,endpoint,**kwargs):
                return {'total_count':1,'jobs':[{'name':'qualification / rust','status':'completed','conclusion':'skipped'}]}
        with self.assertRaises(ValueError):q.require_jobs(Github(),'synthetic/repo',123,1,{'rust'},'qualification / ')

    def test_job_pagination_and_missing_groups(self):
        class Github:
            def api(self,endpoint,**kwargs):
                name='a' if endpoint.endswith('page=1') else 'b'
                return {'total_count':2,'jobs':[{'name':name,'status':'completed','conclusion':'success'}]}
        q.require_jobs(Github(),'synthetic/repo',123,1,{'a','b'})
        with self.assertRaises(ValueError):q.require_jobs(Github(),'synthetic/repo',123,1,{'a','missing'})


class QualifiedGithub(legacy.FakeGithub):
    def api(self,endpoint,**kwargs):
        for name,raw in [('validation-policy.json',POLICY),('conformance-suites.json',SUITES)]:
            if '/contents/scripts/'+name+'?' in endpoint:return {'encoding':'base64','content':base64.b64encode(raw).decode()}
        if '/attempts/' in endpoint and '/jobs?' in endpoint:
            names=q.job_names(evidence()['checks'],json.loads(SUITES))
            return {'total_count':len(names),'jobs':[{'name':'qualification / '+n,'status':'completed','conclusion':'success'} for n in names]}
        return super().api(endpoint,**kwargs)


class QualifiedDistributionTests(unittest.TestCase):
    def setUp(self):
        state=ROOT/'.local/test-state';state.mkdir(parents=True,exist_ok=True)
        self.temp=tempfile.TemporaryDirectory(dir=state);self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        original=fixtures.source_archive
        def source_archive():
            raw=io.BytesIO()
            with tarfile.open(fileobj=io.BytesIO(original()),mode='r:gz') as src,tarfile.open(fileobj=raw,mode='w:gz',format=tarfile.PAX_FORMAT,pax_headers={'comment':fixtures.COMMIT}) as dst:
                for member in src.getmembers():dst.addfile(member,src.extractfile(member))
                member=tarfile.TarInfo('agent-response-gateway/scripts/validation-policy.json');member.mode=0o644;member.size=len(POLICY);dst.addfile(member,io.BytesIO(POLICY))
            return raw.getvalue()
        self.candidate=self.root/'candidate'
        with patch.object(fixtures,'source_archive',side_effect=source_archive):legacy.candidate(self.candidate,fixtures.TARGET)
        self.output=self.root/'distribution'
        provenance.pack_distribution(self.candidate,self.output,evidence())
        (self.output/f'{fixtures.TARGET}.sigstore.jsonl').write_bytes(b'synthetic-proof')

    def test_v2_is_signed_and_verified_without_generic_main_ci_success(self):
        github=QualifiedGithub();github.ci_conclusion='cancelled'
        descriptor=provenance.verify_distribution(github,self.output,fixtures.COMMIT,fixtures.TARGET,123,1,self.root/'state','v0.1.0')
        self.assertEqual(descriptor['schema'],provenance.QUALIFIED_SCHEMA)
        self.assertEqual(descriptor['qualification'],evidence())
        self.assertEqual(len(github.verified),2)

    def test_v2_cannot_drop_qualification_or_change_source_policy_hash(self):
        path=self.output/f'{fixtures.TARGET}.manifest.json';original=json.loads(path.read_text())
        for change in ('missing','hash'):
            value=copy.deepcopy(original)
            if change=='missing':del value['qualification']
            else:value['qualification']['policy_sha256']='0'*64
            path.write_text(json.dumps(value))
            with self.assertRaises((ValueError,provenance.package.PackageError)):
                provenance.inspect_distribution(self.output,fixtures.COMMIT,fixtures.TARGET,self.root/'state')
