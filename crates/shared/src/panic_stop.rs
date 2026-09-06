//! Best-effort stop before vexide's diagnostic panic hook takes over.
//! This cannot handle loss of power, a frozen CPU, or failed VEXos/SDK hardware.
pub fn install() {
    #[cfg(target_os = "vexos")]
    {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // SAFETY: VEXos fills at most V5_MAX_DEVICE_PORTS entries. Only validated
            // motor handles are used. This hook never returns to the panicking control
            // task, and performs no allocation before issuing stop commands.
            unsafe {
                let mut devices =
                    [vex_sdk::V5_DeviceType::kDeviceTypeNoSensor; vex_sdk::V5_MAX_DEVICE_PORTS];
                vex_sdk::vexDeviceGetStatus(devices.as_mut_ptr());
                for (index, kind) in devices.iter().take(21).enumerate() {
                    if *kind == vex_sdk::V5_DeviceType::kDeviceTypeMotorSensor {
                        let device = vex_sdk::vexDeviceGetByIndex(index as u32);
                        vex_sdk::vexDeviceMotorVoltageSet(device, 0);
                        vex_sdk::vexDeviceMotorBrakeModeSet(
                            device,
                            vex_sdk::V5MotorBrakeMode::kV5MotorBrakeModeBrake,
                        );
                        vex_sdk::vexDeviceMotorVelocitySet(device, 0);
                    }
                }
            }
            previous(info);
        }));
    }
}
