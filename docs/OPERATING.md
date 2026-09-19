# Using The Chair

This guide is for the demonstration operator. Judges should drive it from the controller without riding. Keep a spotter nearby and use a clear, level area. Complete the [physical and unloaded checks](COMMISSIONING.md) before powered use.

Before judges use it, raise the drive wheels and check that **both B buttons brake both sides**. If either side keeps driving, stop the demonstration. Repeat the check after any firmware upload or wiring change.

![Start, connect, drive and stop](diagrams/driving-flow.svg)

## Start

1. Put the steering wheel at its physical center and the P17 throttle at its neutral detent **before starting the master program**. Release both wheel buttons and center the controller sticks. The chair uses these positions as its zero points when it starts.
2. Start the **master** program in slot 1, then the **LEFT** and **RIGHT** programs in slots 2 and 3. Powering a Brain and starting its program are separate actions.
3. Wait for both sides to connect and report live motor readings. Once the chair is stopped, calibrated and the controls have been neutral for half a second, it becomes **ON** automatically.
4. If a Brain has not connected within 60 seconds or a motor is still missing after 5 seconds of discovery, the chair shows **E-STOP**. Fix the cause and restart all three programs.

If the wheel or throttle was not centered at startup, stop the chair, center both, then tap **RECENTER**. If the status stays at **WAIT READY**, use the [dashboard checklist](HUD.md#when-it-does-not-become-on).

## Drive

| Mode | How to move | Normal stop |
| --- | --- | --- |
| **WHEEL** | Hold the **left wheel button (ADI A)**, turn P17 forward or backward, and steer with the wheel. | Release the left button. Motors coast. |
| **CONTROLLER** | Move the **left stick up/down** to drive forward/backward and the **right stick left/right** to steer. | Center both sticks. Motors coast. |

WHEEL is selected at startup. Tap **MODE** on the Brain or press controller **X** while stopped with neutral inputs to switch modes. CONTROLLER needs a connected controller. X is ignored while the chair is moving; no live throttle request transfers between modes. P17 does not drive the chair in CONTROLLER mode. Controller A and R1 are unused.

In CONTROLLER mode, holding controller **L1** or the rider's left button coasts the motors. After release, stop and hold the driving controls neutral for half a second before driving again. Losing the controller connection during active controller operation causes an E-STOP; it does not fall back to WHEEL mode.

## B brake

**Press and hold either B button**—the right wheel button (ADI B) or controller B—to request electrical braking on both drive sides. The screen says **BRAKING**. B does not latch a fault. After releasing it, let the chair stop and hold the driving controls neutral for half a second. Then drive normally. A held throttle or stick cannot make it move immediately when B is released.

Electrical braking depends on working Brains, links, motors and power. It is not a mechanical brake or independent power cut.

## Warnings and E-STOP

A yellow **HOT** message warns of a warm motor. Drive power starts reducing at 50°C and reaches zero at 60°C; it returns automatically when the motor cools. Stop and let it cool if the warning appears. **REST ADVISED** follows 120 seconds of driving and clears after a full minute at rest. Neither warning covers the controls or asks for unplugging cables.

A red **E-STOP** means a serious health or control problem, such as a missing Brain or motor, lost link, bad telemetry or failed command. It requests electrical braking and stays latched. Note the reason, correct the cause, then restart **all three programs**. See [fault messages](FAULTS.md).

## Finish

Bring the chair to a complete stop. Stop the programs and power down all three Brains. Before handling wiring, isolate both drive batteries; a cable may backfeed a Brain. Let warm motors cool. Do not carry a passenger until the final chair passes [brake and stopping-distance checks](COMMISSIONING.md).
