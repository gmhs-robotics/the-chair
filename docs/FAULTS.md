# Fault messages and recovery

This index uses the exact labels in `StopReason::label` in `crates/shared/src/safety.rs`. Fatal health or control faults show red E-STOP, request Brake and latch until all three programs restart. The label identifies the first fault, not a complete diagnosis.

## Recovery sequence

1. Stop the demonstration. Release drive enable and return throttle to zero. If software stopping is ineffective, use the independently reviewed physical stopping method.
2. Record the HUD message, mode, node status, temperatures and what happened immediately before the stop. Do this before restarting.
3. Remove all power before handling wiring or obstructed mechanisms. Account for the reported Smart Cable back-powering: removing one battery may not remove power.
4. Correct the cause. Hot motors and the rest reminder are warnings; let motors cool and inspect the drive if heat rises unexpectedly. A battery fault still needs investigation.
5. With drive wheels raised, restart all three programs, master first. Repeat the test that exposed the fault before returning to ground testing.

No button clears a true E-STOP latch. Reconnecting or selecting another mode cannot resume a faulted session. B braking has no latch: release B, stop, and hold drive controls neutral for 500 ms before driving again.

## Message index

| Exact display label | What can trigger it | Check before restarting |
| --- | --- | --- |
| `CONTROLLER LOST` | Missing controller connection/state in CONTROLLER mode | Radio P18, controller power and pairing; no automatic fallback exists |
| `LINK LOST` | Missing Brain after 60 seconds of startup, failed link operation, no valid health through the 500 ms deadline, or loss of readiness while ON | Master P19/LEFT P21 and P20/RIGHT P21; child program status; cable integrity; rising `B` counts identify discarded corrupt/oversized UART frames |
| `PROTOCOL FAULT` | A validly decoded packet has the wrong role, side, session, sequence or command | All three images belong to the same protocol revision; correct endpoints; no stale injector traffic. Isolated corrupt UART frames are discarded and counted, without latching. |
| `COMMAND TIMEOUT` | Child receives no valid next command within its active 150 ms lease | Master program or loop interruption; cable loss; buffered/late traffic |
| `MOTOR / SENSOR FAULT` | Motor missing or invalid for 5 seconds during discovery; later missing/nonfinite motor data, unknown fault bits, or failed motor configuration/output | Repair missing motor and restart all three programs. Current-related `F2`, `F4`, `F8` and temperature `F1` remain warnings. Drive power reduces when hot; VEXos enforces the configured 2.5 A current limit. |
| `BATTERY UNSAFE` | Populated drive-battery telemetry is invalid, reported capacity is below 20%, or another allowed range is exceeded; an absent local pack is accepted only in the documented all-zero/zero-auxiliary case | Each drive node's voltage, capacity, current and temperature; master battery is not checked; shared power arrangement |
| `STEERING FAULT` | Five consecutive invalid telemetry samples, unknown motor fault bits, or a failed steering configuration/output command | P21 motor, encoder ratio, center, travel and cable clearance; current-related `F2`, `F4`, `F8`, finite current magnitude and physical obstruction remain diagnostic only because the configured current/voltage limits constrain the motor; inspect the detailed `[STEERING]` log |
| `INPUT FAULT` | ADI button read error, missing/failed P17 Rotation Sensor, or nonfinite control input | Master ADI A/B, Smart Port 17 and controller axes; negative P17 rotation is valid reverse and finite over-range P17 values clamp to ±100% |
| `CONTROL LOOP STALLED` | A control iteration gap exceeds 100 ms | Blocking operations, excessive work, SDK delays and any attached diagnostic traffic |
| `FIELD CONTROL STOP` | VEXos competition mode is not Driver | Field-control connection/state; motor coast behavior may override application braking |

If no label appears because startup or the display fails, do not interpret the blank screen as permission to drive. Keep the chair unloaded and investigate program diagnostics, power and device setup.

## Stopped without a fault

A normal stop coasts without a red fault. The chair becomes ON automatically after connection, calibration, a full stop and 500 ms of neutral controls. Use [readiness troubleshooting](HUD.md#when-it-does-not-become-on) if it stays at WAIT READY.

Zero throttle or released drive enable coasts the drive motors. Increasing throttle and holding enable can resume motion. B requests electrical Brake while held; it never latches. L1 and the rider's left button in CONTROLLER mode coast while held. Heat and rest advice stay within the normal HUD. Stop the chair and release all controls before changing operators.

For measured stop delays and repeatable fault injection, use the [commissioning record](COMMISSIONING.md). Do not deliberately overheat hardware or inject bad traffic during occupied operation.
