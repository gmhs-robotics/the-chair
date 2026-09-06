# Fault messages and recovery

This index uses the exact labels in `StopReason::label` in `crates/shared/src/safety.rs`. A label identifies the first fault the software latched, not a complete diagnosis. Several different failed readings can share a label.

## Recovery sequence

1. Stop the demonstration. Release drive enable and return throttle to zero. If software stopping is ineffective, use the independently reviewed physical stopping method.
2. Record the HUD message, mode, node status, temperatures and what happened immediately before the stop. Do this before restarting.
3. Remove all power before handling wiring or obstructed mechanisms. Account for the reported Smart Cable back-powering: removing one battery may not remove power.
4. Correct the cause. For thermal or duty faults, provide a full rest and confirm temperatures are below restart thresholds. Restart is not a substitute for cooling.
5. With drive wheels raised, restart all three programs, master first. Repeat the test that exposed the fault before returning to ground testing.

No reset button clears a latch. ARM, reconnecting, releasing an E-stop or selecting another mode cannot resume a faulted session.

## Message index

| Exact display label | What can trigger it | Check before restarting |
| --- | --- | --- |
| `RIDER E-STOP` | Right rider button on ADI B | Rider's reason for stopping; button release and wiring |
| `REMOTE E-STOP` | Controller B while connected | Remote operator's reason for stopping; controller button state |
| `CONTROLLER LOST` | Missing controller connection/state in CONTROLLER mode | Radio P18, controller power and pairing; no automatic fallback exists |
| `LINK LOST` | Failed master link operation, no valid health despite same-sequence retries through the 500 ms deadline, or loss of readiness while armed | Master P19/LEFT P21 and P20/RIGHT P21; child program status; cable integrity; rising `B` counts identify discarded corrupt/oversized UART frames |
| `PROTOCOL FAULT` | Invalid frame, session, side, sequence, command, or child communication failure | All three images belong to the same protocol revision; correct endpoints; no stale injector traffic |
| `COMMAND TIMEOUT` | Child receives no valid next command within its active 150 ms lease | Master program or loop interruption; cable loss; buffered/late traffic |
| `MOTOR / SENSOR FAULT` | After successful startup: missing/nonfinite motor data, unknown fault bits, or failed motor configuration/output | Reconnect a `MISS` motor and wait for valid telemetry. Current-related `F2`, `F4`, `F8` and their combinations remain visible warnings but do not stop motion; VEXos enforces the configured 2.5 A limit. Over-temperature remains a dedicated thermal stop. |
| `MOTOR HOT - COOL DOWN` | Motor reaches 50°C during operation | All motors at or below 40°C and batteries below 38°C before restarting; a motor above 40°C during startup remains safely parked until cool |
| `BATTERY UNSAFE` | Populated battery telemetry is invalid or outside its allowed range; zero auxiliary fields with zero or valid voltage is accepted as no local pack | Each drive node's voltage, capacity, current and temperature; master battery is not checked; shared power arrangement |
| `DUTY LIMIT - REST` | 120 seconds cumulative drive activity without a full 60-second rest | Rest and temperatures; brief button releases do not restore budget |
| `STEERING FAULT` | Five consecutive invalid telemetry samples, unknown motor fault bits, or a failed steering configuration/output command | P21 motor, encoder ratio, center, travel and cable clearance; current-related `F2`, `F4`, `F8`, finite current magnitude and physical obstruction remain diagnostic only because the configured current/voltage limits constrain the motor; inspect the detailed `[STEERING]` log |
| `INPUT FAULT` | ADI button read error, missing/failed P17 Rotation Sensor, or nonfinite control input | Master ADI A/B, Smart Port 17 and controller axes; negative P17 rotation is valid reverse and finite over-range P17 values clamp to ±100% |
| `CONTROL LOOP STALLED` | A control iteration gap exceeds 100 ms | Blocking operations, excessive work, SDK delays and any attached diagnostic traffic |
| `FIELD CONTROL STOP` | VEXos competition mode is not Driver | Field-control connection/state; motor coast behavior may override application braking |

If no label appears because startup or the display fails, do not interpret the blank screen as permission to drive. Keep the chair unloaded and investigate program diagnostics, power and device setup.

## Braked or parked without a fault

A normal stop can disarm without a red fault. Return all controls to neutral, stop, wait 500 ms, then ARM. Use [ARM troubleshooting](HUD.md#when-arm-does-nothing) if it stays parked.

Zero throttle or released drive enable coasts the drive motors, but the system remains armed and steering feedback remains active. Increasing throttle and holding enable can resume motion. Use PARK or a mode-specific disarm control before changing operators.

For measured stop delays and repeatable fault injection, use the [commissioning record](COMMISSIONING.md). Do not deliberately overheat hardware or inject bad traffic during occupied operation.
