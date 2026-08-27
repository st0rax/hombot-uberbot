"""Evaluate motion-readiness evidence without talking to the robot.

This tool is intentionally only an evaluator. It cannot deploy software,
open a device connection or send a movement command. Its successful result is
``READY_FOR_BOUNDED_FIRST_MOTION``; every missing, failed, malformed, future
or stale receipt produces ``LOCKED``.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Mapping


SCHEMA_VERSION = 1
MAX_FUTURE_SKEW_SECONDS = 5 * 60
MAX_AGE_SECONDS_BY_GATE = {
    "A": 60 * 60,
    "B": 60 * 60,
    "C": 15 * 60,
    "D": 15 * 60,
    "E": 2 * 60,
}

# Keep these IDs stable: evidence files and STATUS_LIVE.md receipts refer to
# them. Changing the safety contract requires a schema-version increment.
REQUIRED_CHECKS = (
    ("A", "baseline.deployed_build", "deployed build and configuration known"),
    ("A", "baseline.ssh_rollback", "key SSH and rollback path verified"),
    ("A", "baseline.rc_local_executable", "rc.local executable bit verified"),
    ("A", "baseline.no_pending_change", "no pending maintenance or deployment"),
    ("A", "baseline.lg_safety_active", "LG/Micom safety authority active"),
    ("B", "function.inventory_complete", "implemented function inventory complete"),
    ("B", "function.health_system", "health and system endpoints pass"),
    ("B", "function.smartcontrol", "SmartControl status passes"),
    ("B", "function.camera", "supported camera modes pass"),
    ("B", "function.rawsensor_fresh", "RawSensor is available and fresh"),
    ("B", "function.auth_negative", "unauthorized writes fail closed"),
    ("B", "function.auth_positive", "authorized writes reach intended handlers"),
    ("B", "function.resource_limits", "resource limits pass"),
    ("B", "function.watchdog", "watchdog behavior passes"),
    ("B", "function.connection_bounds", "connection bounds pass"),
    ("B", "function.logs_clean", "logs show no unexplained fault"),
    ("C", "sensor.inventory_complete", "motion-safety sensor inventory complete"),
    ("C", "sensor.bumpers", "every bumper transitions and releases"),
    ("C", "sensor.cliff", "every cliff sensor transitions and recovers"),
    ("C", "sensor.wheel_drop", "wheel-drop or lift sensors pass"),
    ("C", "sensor.cover_contact", "relevant cover contacts pass"),
    ("C", "sensor.dock_power", "dock and relevant power state pass"),
    ("C", "sensor.thermal", "thermal safety state passes"),
    ("C", "sensor.contradictory_fail_closed", "contradictory inputs fail closed"),
    ("C", "sensor.stale_fail_closed", "stale sensor feed inhibits motion"),
    ("D", "control.semantic_allowlist", "semantic command allowlist enforced"),
    ("D", "control.exclusive_lease", "exclusive short-lived lease enforced"),
    ("D", "control.heartbeat_stop", "heartbeat expiry stops on device"),
    ("D", "control.disconnect_stop", "controller disconnect stops on device"),
    ("D", "control.transport_stop", "LG transport loss stops on device"),
    ("D", "control.sensor_trip_stop", "live safety trip stops on device"),
    ("D", "control.stale_sensor_stop", "stale sensor stops active motion"),
    ("D", "control.independent_stop", "stop is independent of external Brain"),
    ("D", "control.stop_latency", "measured stop latency is within bound"),
    ("E", "envelope.area_clear", "controlled first-motion area is clear"),
    ("E", "envelope.action_bounded", "first action has bounded speed and duration"),
    ("E", "envelope.stop_ready", "independent stop path is ready"),
)


@dataclass(frozen=True)
class CheckResult:
    gate: str
    check_id: str
    label: str
    passed: bool
    reason: str


@dataclass(frozen=True)
class GateResult:
    ready: bool
    session_id: str
    deployed_version: str
    checks: tuple[CheckResult, ...]


def _aware_timestamp(value: Any) -> datetime | None:
    if not isinstance(value, str) or not value.strip():
        return None
    try:
        parsed = datetime.fromisoformat(value.strip().replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        return None
    return parsed.astimezone(timezone.utc)


def _text(value: Any) -> str:
    return value.strip() if isinstance(value, str) else ""


def evaluate(
    document: Mapping[str, Any],
    *,
    now: datetime | None = None,
) -> GateResult:
    """Return a fail-closed evaluation of one evidence document."""

    current = now or datetime.now(timezone.utc)
    if current.tzinfo is None or current.utcoffset() is None:
        raise ValueError("now must be timezone-aware")
    current = current.astimezone(timezone.utc)

    session_id = _text(document.get("session_id"))
    deployed_version = _text(document.get("deployed_version"))
    schema_ok = document.get("schema_version") == SCHEMA_VERSION
    checks_value = document.get("checks")
    checks: Mapping[str, Any] = checks_value if isinstance(checks_value, Mapping) else {}

    results: list[CheckResult] = []
    for gate, check_id, label in REQUIRED_CHECKS:
        receipt = checks.get(check_id)
        reasons: list[str] = []
        if not schema_ok:
            reasons.append(f"schema_version must be {SCHEMA_VERSION}")
        if not session_id:
            reasons.append("document session_id missing")
        if not deployed_version:
            reasons.append("document deployed_version missing")
        if not isinstance(receipt, Mapping):
            reasons.append("receipt missing")
        else:
            status = _text(receipt.get("status")).lower()
            if status != "pass":
                reasons.append(f"status is {status or 'missing'}, not pass")
            if _text(receipt.get("session_id")) != session_id:
                reasons.append("session_id mismatch")
            if _text(receipt.get("deployed_version")) != deployed_version:
                reasons.append("deployed_version mismatch")
            observed_at = _aware_timestamp(receipt.get("observed_at"))
            if observed_at is None:
                reasons.append("observed_at missing, invalid or lacks timezone")
            else:
                age = current - observed_at
                max_age_seconds = MAX_AGE_SECONDS_BY_GATE[gate]
                if age > timedelta(seconds=max_age_seconds):
                    reasons.append(
                        "receipt stale: "
                        f"{int(age.total_seconds())}s > {max_age_seconds}s gate limit"
                    )
                if age < -timedelta(seconds=MAX_FUTURE_SKEW_SECONDS):
                    reasons.append("observed_at is too far in the future")
            if not _text(receipt.get("evidence")):
                reasons.append("evidence reference missing")

        results.append(
            CheckResult(
                gate=gate,
                check_id=check_id,
                label=label,
                passed=not reasons,
                reason="; ".join(reasons) if reasons else "fresh positive receipt",
            )
        )

    return GateResult(
        ready=all(result.passed for result in results),
        session_id=session_id,
        deployed_version=deployed_version,
        checks=tuple(results),
    )


def template() -> dict[str, Any]:
    """Return a pending template containing every required stable check ID."""

    return {
        "schema_version": SCHEMA_VERSION,
        "session_id": "REPLACE_WITH_ONE_TEST_SESSION_ID",
        "deployed_version": "REPLACE_WITH_EXACT_DEPLOYED_VERSION",
        "checks": {
            check_id: {
                "status": "pending",
                "observed_at": None,
                "session_id": "REPLACE_WITH_ONE_TEST_SESSION_ID",
                "deployed_version": "REPLACE_WITH_EXACT_DEPLOYED_VERSION",
                "evidence": "",
            }
            for _, check_id, _ in REQUIRED_CHECKS
        },
    }


def report_dict(result: GateResult) -> dict[str, Any]:
    return {
        "motion_gate": "READY_FOR_BOUNDED_FIRST_MOTION" if result.ready else "LOCKED",
        "session_id": result.session_id,
        "deployed_version": result.deployed_version,
        "passed": sum(check.passed for check in result.checks),
        "required": len(result.checks),
        "checks": [
            {
                "gate": check.gate,
                "id": check.check_id,
                "status": "PASS" if check.passed else "LOCKED",
                "reason": check.reason,
            }
            for check in result.checks
        ],
    }


def _print_text(result: GateResult) -> None:
    state = "READY_FOR_BOUNDED_FIRST_MOTION" if result.ready else "LOCKED"
    passed = sum(check.passed for check in result.checks)
    print(f"MOTION GATE: {state}")
    print(f"SESSION: {result.session_id or '(missing)'}")
    print(f"DEPLOYED VERSION: {result.deployed_version or '(missing)'}")
    print(f"CHECKS: {passed}/{len(result.checks)} passed")
    for check in result.checks:
        marker = "PASS" if check.passed else "LOCKED"
        print(f"[{marker}] {check.gate} {check.check_id}: {check.reason}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Fail-closed evaluator for HomBot motion-readiness evidence."
    )
    parser.add_argument("evidence", nargs="?", type=Path, help="evidence JSON file")
    parser.add_argument("--template", action="store_true", help="print a pending JSON template")
    parser.add_argument("--json", action="store_true", help="print the evaluation as JSON")
    args = parser.parse_args(argv)

    if args.template:
        if args.evidence:
            parser.error("evidence cannot be combined with --template")
        print(json.dumps(template(), indent=2))
        return 0
    if args.evidence is None:
        parser.error("evidence is required unless --template is used")

    try:
        with args.evidence.open("r", encoding="utf-8") as handle:
            document = json.load(handle)
        if not isinstance(document, Mapping):
            raise ValueError("top-level JSON value must be an object")
        result = evaluate(document)
    except (OSError, json.JSONDecodeError, ValueError) as exc:
        print(f"MOTION GATE: LOCKED\nINVALID EVIDENCE: {exc}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps(report_dict(result), indent=2))
    else:
        _print_text(result)
    return 0 if result.ready else 2


if __name__ == "__main__":
    raise SystemExit(main())
