#!/usr/bin/env python3
"""Summarize observed Actions timing without treating runner-seconds as billed usage."""
import argparse
from datetime import datetime
import json
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[1]


def elapsed(start,end):
    if not start or not end or start.startswith('0001-') or end.startswith('0001-'):return None
    value=(datetime.fromisoformat(end.replace('Z','+00:00'))-datetime.fromisoformat(start.replace('Z','+00:00'))).total_seconds()
    return round(value,3) if value>=0 else None


def summarize(run):
    jobs=[];gate=None
    for job in run['jobs']:
        duration=elapsed(job.get('startedAt'),job.get('completedAt')) if job['status']=='completed' and job['conclusion']!='skipped' else None
        steps=[{'name':s['name'],'conclusion':s['conclusion'],'seconds':elapsed(s.get('startedAt'),s.get('completedAt'))} for s in job.get('steps',[]) if s.get('status')=='completed' and s.get('conclusion')!='skipped']
        jobs.append(dict(name=job['name'],status=job['status'],conclusion=job['conclusion'],seconds=duration,steps=steps))
        if job['name']=='ci-required' and job['status']=='completed':gate=elapsed(run['createdAt'],job.get('completedAt'))
    known=[j['seconds'] for j in jobs if j['seconds'] is not None]
    return dict(schema='gateway-validation-metrics/v1',run_id=run['databaseId'],head_sha=run['headSha'],event=run['event'],url=run['url'],
                status=run['status'],conclusion=run['conclusion'],required_gate_seconds=gate,
                completed_job_seconds_sum=round(sum(known),3) if known else None,
                measured_jobs=len(known),jobs=jobs,
                interpretation='Observed wall times include job overhead. Sum is not billed usage; queue, cache and concurrency conditions can differ.')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('run_id',type=int);parser.add_argument('--output',type=Path);args=parser.parse_args()
    fields='databaseId,headSha,event,url,status,conclusion,createdAt,jobs'
    run=json.loads(subprocess.check_output(['gh','run','view',str(args.run_id),'--json',fields],cwd=ROOT,text=True))
    raw=json.dumps(summarize(run),indent=2)+'\n'
    if args.output:args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(raw)
    print(raw,end='')
