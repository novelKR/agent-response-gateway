"""Keep queued/unknown timings distinct from zero and deployment waits from the CI gate."""
from pathlib import Path
import sys
import unittest
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'scripts'))
import validation_metrics as metrics


class MetricsTests(unittest.TestCase):
    def run_data(self):
        return dict(databaseId=123,attempt=1,headSha='a'*40,event='push',url='https://example.invalid/run',status='waiting',conclusion='',createdAt='2026-01-01T00:00:00Z',jobs=[
            dict(name='ci-required',status='completed',conclusion='success',startedAt='2026-01-01T00:00:10Z',completedAt='2026-01-01T00:00:15Z',steps=[]),
            dict(name='docs-pages / deploy',status='waiting',conclusion='',startedAt='0001-01-01T00:00:00Z',completedAt=None,steps=[]),
            dict(name='unneeded',status='completed',conclusion='skipped',startedAt='2026-01-01T00:00:15Z',completedAt='2026-01-01T00:00:15Z',steps=[])])

    def test_gate_success_and_deployment_wait_are_separate(self):
        report=metrics.summarize(self.run_data())
        self.assertEqual(report['status'],'waiting')
        self.assertEqual(report['required_gate_seconds'],15)
        self.assertEqual(report['completed_job_seconds_sum'],5)
        self.assertEqual(report['measured_jobs'],1)
        self.assertIsNone(report['jobs'][1]['seconds'])
        self.assertIsNone(report['jobs'][2]['seconds'])

    def test_no_completed_jobs_has_unavailable_total(self):
        run=self.run_data();run['jobs']=run['jobs'][1:]
        report=metrics.summarize(run)
        self.assertIsNone(report['completed_job_seconds_sum'])
        self.assertIsNone(report['required_gate_seconds'])

    def test_cli_steps_without_status_and_rerun_are_preserved(self):
        run=self.run_data();run['attempt']=2
        run['jobs'][0]['steps']=[dict(name='Prepare pinned tools',conclusion='success',startedAt='2026-01-01T00:00:10Z',completedAt='2026-01-01T00:00:13Z')]
        report=metrics.summarize(run)
        self.assertEqual(report['attempt'],2)
        self.assertTrue(report['rerun'])
        self.assertEqual(report['jobs'][0]['steps'][0]['seconds'],3)
