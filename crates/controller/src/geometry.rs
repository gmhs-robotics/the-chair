//! Commissioning assumptions, not dimensions inferred from a photograph.
//! Measure the complete gear train with the drive wheels raised.
/// Motor encoder revolutions per physical steering-wheel revolution.
pub const ENCODER_PER_WHEEL: f64 = 1.0;
/// Measured wheel travel from manually calibrated center, in each direction.
pub const MAX_STEERING_DEG: f64 = 80.0;
pub const STEERING_DEADZONE_DEG: f64 = 10.0;
