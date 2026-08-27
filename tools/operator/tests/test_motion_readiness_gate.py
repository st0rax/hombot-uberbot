import copy
import sys
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path


OPERATOR_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(OPERATOR_DIR))

import motion_readiness_gate as gate  # noqa: E402


NOW = datetime(2026, 8, 27, 18, 0, tzinfo=timezone.utc)


def passing_document():
    document = gate.template()
    document["session_id"] = "session-20260827-a"
    document["deployed_version"] = "0.1.11+example"
    for receipt in document["checks"].values():
        receipt.update(
            {
                "status": "pass",
                "observed_at": (NOW - timedelta(seconds=30)).isoformat(),
                "session_id": document["session_id"],
                "deployed_version": document["deployed_version"],
                "evidence": "test receipt",
            }
        )
    return document


class MotionReadinessGateTests(unittest.TestCase):
    def test_all_fresh_positive_receipts_release_only_the_bounded_gate(self):
        result = gate.evaluate(passing_document(), now=NOW)

        self.assertTrue(result.ready)
        self.assertEqual(len(result.checks), len(gate.REQUIRED_CHECKS))
        self.assertEqual(
            gate.report_dict(result)["motion_gate"],
            "READY_FOR_BOUNDED_FIRST_MOTION",
        )

    def test_pending_template_is_locked(self):
        result = gate.evaluate(gate.template(), now=NOW)

        self.assertFalse(result.ready)
        self.assertTrue(all(not check.passed for check in result.checks))

    def test_one_missing_receipt_locks_the_whole_gate(self):
        document = passing_document()
        del document["checks"]["sensor.bumpers"]

        result = gate.evaluate(document, now=NOW)

        self.assertFalse(result.ready)
        bumper = next(c for c in result.checks if c.check_id == "sensor.bumpers")
        self.assertIn("receipt missing", bumper.reason)

    def test_one_failed_receipt_locks_the_whole_gate(self):
        document = passing_document()
        document["checks"]["sensor.cliff"]["status"] = "fail"

        result = gate.evaluate(document, now=NOW)

        self.assertFalse(result.ready)
        cliff = next(c for c in result.checks if c.check_id == "sensor.cliff")
        self.assertIn("not pass", cliff.reason)

    def test_stale_receipt_locks_the_whole_gate(self):
        document = passing_document()
        document["checks"]["control.heartbeat_stop"]["observed_at"] = (
            NOW - timedelta(minutes=16)
        ).isoformat()

        result = gate.evaluate(document, now=NOW)

        self.assertFalse(result.ready)
        heartbeat = next(
            c for c in result.checks if c.check_id == "control.heartbeat_stop"
        )
        self.assertIn("stale", heartbeat.reason)

    def test_freshness_is_stricter_near_first_motion(self):
        document = passing_document()
        observed_at = (NOW - timedelta(minutes=3)).isoformat()
        document["checks"]["baseline.deployed_build"]["observed_at"] = observed_at
        document["checks"]["envelope.area_clear"]["observed_at"] = observed_at

        result = gate.evaluate(document, now=NOW)

        reasons = {check.check_id: check for check in result.checks}
        self.assertTrue(reasons["baseline.deployed_build"].passed)
        self.assertFalse(reasons["envelope.area_clear"].passed)
        self.assertIn("120s gate limit", reasons["envelope.area_clear"].reason)

    def test_mixed_session_or_version_locks_the_whole_gate(self):
        document = passing_document()
        document["checks"]["function.camera"]["session_id"] = "older-session"
        document["checks"]["function.smartcontrol"]["deployed_version"] = "0.1.10"

        result = gate.evaluate(document, now=NOW)

        self.assertFalse(result.ready)
        reasons = {check.check_id: check.reason for check in result.checks}
        self.assertIn("session_id mismatch", reasons["function.camera"])
        self.assertIn("deployed_version mismatch", reasons["function.smartcontrol"])

    def test_naive_or_far_future_timestamp_locks_the_whole_gate(self):
        document = passing_document()
        document["checks"]["baseline.deployed_build"]["observed_at"] = (
            NOW - timedelta(seconds=10)
        ).replace(tzinfo=None).isoformat()
        document["checks"]["baseline.ssh_rollback"]["observed_at"] = (
            NOW + timedelta(minutes=6)
        ).isoformat()

        result = gate.evaluate(document, now=NOW)

        self.assertFalse(result.ready)
        reasons = {check.check_id: check.reason for check in result.checks}
        self.assertIn("lacks timezone", reasons["baseline.deployed_build"])
        self.assertIn("too far in the future", reasons["baseline.ssh_rollback"])

    def test_wrong_schema_locks_every_check(self):
        document = passing_document()
        document["schema_version"] = 99

        result = gate.evaluate(document, now=NOW)

        self.assertFalse(result.ready)
        self.assertTrue(all("schema_version" in check.reason for check in result.checks))

    def test_input_is_not_mutated(self):
        document = passing_document()
        before = copy.deepcopy(document)

        gate.evaluate(document, now=NOW)

        self.assertEqual(document, before)


if __name__ == "__main__":
    unittest.main()
