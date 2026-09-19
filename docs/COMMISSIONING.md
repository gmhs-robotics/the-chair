# Commissioning and test record

This firmware is for an experimental club demonstrator. Passing host tests and producing a V5 binary does not establish that the chair is safe to carry someone. Record the results below with a responsible adult/mentor before occupied use. Do not use a rider as test ballast.

Work in order: identify the build, verify wiring with power removed, raise the wheels, calibrate, inject faults, then perform unoccupied ground tests. A failed check stops progression to the next stage.

## Record this build

Copy this table for each hardware or firmware revision. Blank fields mean **not verified**, not passed.

| Record | Value |
| --- | --- |
| Date / operator / mentor | |
| Firmware revision and three binary SHA-256 hashes | |
| Brain identifiers and upload slots | |
| Actual motor cartridges and per-port directions | |
| Wheel circumference and complete drive ratio | |
| Steering ratio, center mark and safe travel | |
| P17 Rotation Sensor zero / full / disconnected | |
| Battery arrangement and independent power-cut check | |
| Ambient conditions / ballast / surface | |

Use the [cable schedule](WIRING.md#cable-schedule) and [operator sequence](OPERATING.md#start) throughout. Do not change a limit to make a failing test appear to pass.

## Before any motion

- Secure the chair with **all drive wheels raised**, clear of people, cables and loose clothing. Keep an independent means of removing **all** motor power within reach.
- Check the reported shared Smart Cable power arrangement. Two battery packs and back-powered Brains require electrical review; this software does not establish compatibility or isolate them. Verify that a supposed power-off actually removes power from every node.
- Verify that the steering assembly cannot pinch hands or wind up cables over its complete travel. Confirm mounting, gears, guards and braking mechanically. Software cannot inspect these.
- Upload all three new images. Start master slot 1 before manually starting left slot 2 and right slot 3. Upload does not start programs.
- Confirm P19/LEFT/P21 and P20/RIGHT/P21; each drive Brain's bottom motors on P4/P5 and top motors on P6/P7; steering P21; throttle Rotation Sensor P17; radio P18; and buttons ADI A/B. Leave ADI C empty.

## Calibration and direction checks

1. Put the P17 throttle at mechanical zero and leave buttons released. Observe that no propulsion occurs during startup, handshake or missing-node waits. Each child should handshake first, then show `INITIALIZING MOTORS`; all P4–P7 states must remain `OK` for 300 ms before link readiness. Start both child programs within 60 seconds. A missing Brain at 60 seconds or a motor still missing 5 seconds into discovery must latch E-STOP and require all three programs to restart after repair.
2. Confirm ADI C is empty. Tap CENTER with P17 at its neutral detent, then rotate it slowly both ways. Verify −8°…8° maps to 0%, each nonzero request rounds away from zero in 10% steps, intended forward travel reaches +270°/+100%, and intended reverse reaches −270°/−100%. Adjust measured constants if the installed mechanism differs; never widen fault limits merely to hide bad geometry.
3. With power off, mark physical steering center and verify the encoder-to-wheel gear ratio and safe angular range. Enter those values in `geometry.rs`. Current settings are 1:1, a 10° deadzone and ±80° travel with a blue cartridge. Verify a physical right turn moves the HUD wheel right and produces a positive steering value. Motion beyond the nominal range clamps the steering input instead of latching a fault.
4. Start again with the wheel physically centered and still, and P17 at its neutral detent. The master sets both zero positions automatically; RECENTER can correct them only when the chair is stopped. Once both nodes have healthy telemetry, turn the wheel by hand: verify angle sign, smoothness and configured limit. Re-centering must not drift as the wheel crosses zero repeatedly.
5. Keep controls neutral for at least 500 ms; the status becomes ON automatically. Test feedback at low force: it must resist displacement and damp motion, not accelerate outward. If direction or gear ratio is wrong, stop and fix it; do not compensate by increasing feedback strength.
6. Lift the drive wheels clear, hold the left button and apply only a small positive P17 request. Verify **all four motors on each side propel the same physical forward direction through the gears**. Return P17 to neutral, wait for Coast, then apply a small negative request and verify all eight reverse. LEFT P4/P5 use Reverse and P6/P7 Forward; RIGHT P4/P5 use Forward and P6/P7 Reverse as base polarity. Stop immediately if any pair fights its gear train.
7. Turn right at low request: left side must run faster than right. Repeat left. With P17 at zero, turning the wheel must command equal/opposite side motion. At full steering, verify both sides reach full power in opposite directions. Record wheel RPM and actual wheel circumference/external gearing before relying on MPH.
8. With wheels still raised, test P17 endpoints. Verify commanded acceleration, 12 V ceiling, 2.5 A per-motor current limits and speed feedback. Finite RPM readings are diagnostic/control inputs and do not latch a fault. A voltage ceiling is not a ground-speed governor.

## Fault-injection matrix

Run each test in WHEEL and CONTROLLER where applicable. Keep wheels raised throughout. Record measured stop delay, every motor's behavior and HUD state. Restart all three programs between true latched-fault tests. B brake and heat-warning tests must recover without restarting.

For each row, record: **mode, stimulus, observed stop delay, LEFT result, RIGHT result, steering result, HUD reason, restart required, pass/fail, evidence**. Use host-side injection for synthetic protocol/telemetry faults; never connect a fault injector during occupied operation.

| Test | Expected result |
| --- | --- |
| Boot with drive enable held | Remains at WAIT READY; becomes ON only after release and 500 ms neutral |
| Start child before master | Child stays braked, awaiting assignment |
| Start only one child | Master initially shows `CONNECTED; WAIT PEER`; connected side receives no health request or zero command; after 60 seconds E-STOP latches |
| Leave one child absent for 60 seconds | Master latches `LINK LOST` E-STOP; late child startup cannot clear it |
| Leave one drive motor absent for 5 seconds after first zero command | Child latches `MOTOR / SENSOR FAULT` E-STOP; reconnection cannot clear it |
| Left wheel button release while driving | Removes voltage and coasts immediately; pressing again resumes |
| Right wheel button press | Both sides request electrical Brake while held; release, stop and hold neutral 500 ms to resume without reboot |
| Controller B | Same momentary electrical Brake in either mode while connected; no latch |
| Controller L1 or rider left button in CONTROLLER | Coasts while held; after release, stop and hold neutral 500 ms before motion resumes |
| Controller A and R1 | Unused; no effect on motion authority |
| Negative P17 in WHEEL / downward controller stick / steering with zero throttle | Both modes reverse; either steering input commands equal/opposite in-place turning; full steering reaches full side power |
| Mode change while moving | Ignored; no transfer of nonzero command. While stopped and neutral, MODE/X switches modes and requires another 500 ms neutral before motion |
| Controller powers off or radio unplugs in CONTROLLER | Controller-loss fault stop; no fallback |
| Controller absent in WHEEL | Wheel mode remains available |
| Unplug LEFT or RIGHT serial cable | Disconnected child stops on command lease; master stops other side on health loss; reconnect cannot resume |
| Stop/restart master program while children run | Children latch command/session loss; restarting master alone cannot resume |
| Restart one child | Missing assignment/health trips system; no one-side restart into motion |
| Disconnect one motor or steering motor | Fault on missing/invalid telemetry, all drive commands stop |
| Controlled steering obstruction using a fixture, hands clear | Low current/voltage cap remains enforced; obstruction/current warnings stay visible without a Steering fault; stop if temperature rises unexpectedly |
| Send malformed, oversized, wrong-version or replayed serial frame | Protocol fault / no authorized motion |
| Pause control servicing beyond 100 ms / command stream beyond 150 ms | Loop-stall or command-timeout latch before late commands can revive motion |
| Trigger a deliberate software panic on an unloaded node | Panic hook requests local motor Brake; peer supervision stops the other side |
| Inject hot/invalid health values in tests | Motor ≥50°C shows warning and starts derating, reaching zero output at 60°C; battery ≥45°C or missing/nonfinite telemetry latches |
| Run drive requests for 120 seconds without 60 seconds at rest | HUD shows `REST ADVISED`; no latch or motion lockout |
| Start with warm motors or warm battery | Motor heat does not latch; drive stays power limited until cool. Battery must be <38°C before ON |

Do not deliberately overheat a battery or motor. Use simulated telemetry for threshold tests and verify real temperature reporting during normal low-load operation.

Also start each child while its master is already sending, so it may initially receive a partial frame. Verify handshake retries while braked and a dropped assignment ACK can be retried before the first zero command. Once a command lease is active, malformed traffic must latch and duplicate handshakes must not extend it. Verify that a simulated transient missing motor recovers before 5 seconds without granting motion; persistent absence must latch. Four healthy motors must remain stable for 300 ms before readiness.

## Ground testing

Only after unloaded checks pass, test at the lowest request on a flat, dry, clear surface with the chair unoccupied and a spotter. Increase ballast gradually. Measure ordinary Coast stopping distance and fault-Brake stopping distance under the worst expected load, speed and surface; do not assume either holds on a slope. Record motor/pack temperatures after repeated runs in representative outdoor conditions.

The duty counter is advisory. Short stops do not clear it; a full 60 seconds at rest does. Heat warnings and power reduction clear automatically as motors cool. Investigate unexpected heat and record temperatures during repeated runs. Do not blow compressed air into electronics or batteries as a substitute for proper thermal management.

## Limits of software protection

- The two ordinary bumper buttons are **not** a redundant, safety-rated stop circuit. An open right-button wire can look exactly like a released button. Successful self-tests do not change that electrical limitation.
- Ordinary stops deliberately Coast. Fault Brake requests are electrical commands; neither behavior guarantees holding, stopping distance or operation after power/SDK failure. VEXos field-disabled mode can force motor coast despite application Brake requests.
- The 10 ms local loop, 40/150 ms command schedule/lease and 100/500 ms health schedule/deadline provide bounded application intentions, not hard real-time certification. Child command loss stops locally before the slower master health deadline. A late loop latches when execution resumes.
- A frozen drive CPU, failed SDK, corrupted exception handler, motor-controller failure or loss of power may prevent local braking. The panic hook helps ordinary Rust panics; it is not a hardware watchdog. A peer cannot electrically shut off another Brain's motors.
- Controller freshness is limited to VEXos detection. Encoder speed is not ground speed; slip, gearing, downhill motion or an incorrect sign invalidate assumptions. Test the final loaded mechanics.
- A timestamp session and CRC protect against accidental stale/corrupt traffic, not a malicious device. Do not attach untrusted equipment to live drive links.
- Software cannot turn other Brains on after master, isolate the owner's shared battery path, verify that all power is removed, or provide a safe two-node mode after master/rider-button removal.

Use an independently reviewed means to cut all drive power and a suitable physical braking strategy before carrying a person. Keep the master, rider buttons and radio attached for this firmware. This software pass does not certify the mechanical or power design.
