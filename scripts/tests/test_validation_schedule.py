"""The fixed daily full schedule must remain separate from integration cancellation."""
from pathlib import Path
import unittest
ROOT=Path(__file__).resolve().parents[2]


class ScheduleTests(unittest.TestCase):
    def test_daily_utc_schedule_and_manual_entry_remain(self):
        source=(ROOT/'.github/workflows/ci.yml').read_text()
        header=source.split('jobs:\n',1)[0]
        self.assertIn("cron: '17 18 * * *'",header)
        self.assertIn('workflow_dispatch:',header)
        self.assertIn("inputs.release_profile && 'qualification' || github.event_name",header)
        self.assertIn('!inputs.release_profile &&',header)
        self.assertNotIn("github.event_name == 'schedule'",header)
        self.assertNotIn('paths-ignore:',header)
