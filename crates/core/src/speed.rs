use crate::config::Config;

/// Apply speed multiplier and sigmoid acceleration curve to a delta vector.
/// Returns (dx, dy) with speed and acceleration applied.
///
/// Ported from DistanceManager.ApplySpeedAndAcc().
pub fn apply_speed_and_accel(
    device_id: &str,
    config: &Config,
    delta: (f32, f32),
    elapsed_ms: i64,
) -> (f32, f32) {
    let device_config = config.device_config(device_id);

    // Apply speed multiplier: speed=60 → 1.0x, speed=120 → 2.0x
    let speed = device_config.cursor_speed / 60.0;
    let mut dx = delta.0 * speed;
    let mut dy = delta.1 * speed;

    // Calculate mouse velocity: relative speed between 0 and 4
    let dist = (dx * dx + dy * dy).sqrt();
    let mut mouse_velocity = if elapsed_ms > 0 {
        (dist / elapsed_ms as f32).min(4.0)
    } else {
        1.0
    };
    if mouse_velocity.is_nan() || mouse_velocity.is_infinite() {
        mouse_velocity = 1.0;
    }

    // Apply acceleration curve
    let a = device_config.cursor_acceleration / 10.0;
    let pointer_velocity = if a != 0.0 {
        // Sigmoid acceleration: maps slow → ~0.7x, fast → ~1.5x
        // See https://www.desmos.com/calculator/khtj85jopn
        let z = 0.8_f64;
        let offset = (3.0 - (z / 0.3 - 1.0).log2()) / (2.6 * a as f64);
        let sigmoid_input = 2.6 * a as f64 * (mouse_velocity as f64 - 1.0 + offset) - 3.0;
        (0.7 + 0.8 * sigmoid(sigmoid_input)) as f32
    } else {
        1.0
    };

    dx *= pointer_velocity;
    dy *= pointer_velocity;

    (dx, dy)
}

/// Apply speed multiplier only (no acceleration).
/// Used by FingerCounter for threshold calculations.
pub fn apply_speed(device_id: &str, config: &Config, distance: f32) -> f32 {
    let device_config = config.device_config(device_id);
    distance * (device_config.cursor_speed / 60.0)
}

/// Sigmoid function: 1 / (1 + e^-x)
fn sigmoid(x: f64) -> f64 {
    let k = x.exp();
    k / (1.0 + k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeviceDragConfig;

    #[test]
    fn test_sigmoid_range() {
        // Sigmoid should output between 0 and 1
        for i in -20..=20 {
            let x = i as f64 * 0.5;
            let s = sigmoid(x);
            assert!(s >= 0.0 && s <= 1.0, "sigmoid({}) = {}", x, s);
        }
    }

    #[test]
    fn test_speed_and_accel_output_range() {
        let mut config = Config::default();
        let mut dc = DeviceDragConfig::default();
        dc.cursor_speed = 30.0;
        config.device_configs.insert("test".into(), dc);

        // With default config (speed=30, accel=10), output should be reasonable
        let (dx, dy) = apply_speed_and_accel("test", &config, (10.0, 10.0), 10);
        assert!(dx.is_finite() && dy.is_finite());
        // Output should be scaled down
        assert!((dx * dx + dy * dy).sqrt() < 20.0);
    }

    #[test]
    fn test_apply_speed() {
        let mut config = Config::default();
        let mut dc = DeviceDragConfig::default();
        dc.cursor_speed = 30.0;
        config.device_configs.insert("test".into(), dc);

        // Default cursor_speed = 30, so multiplier = 30/60 = 0.5
        let result = apply_speed("test", &config, 100.0);
        assert!((result - 50.0).abs() < 0.01);
    }
}
