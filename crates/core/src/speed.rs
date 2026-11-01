use crate::config::Config;

/// Stateful dynamic velocity and trajectory tracker for three-finger drag.
///
/// Implements:
/// 1. Asymmetric EMA velocity envelope (fast attack + smooth decay) with
///    instantaneous positive-acceleration (jerk) boost.
/// 2. Three-stage macOS-style dynamic acceleration curve:
///    - Low-speed precision micro-control zone
///    - Mid-speed ergonomic 1:1 cruise zone
///    - High-speed progressive cross-screen boost zone
/// 3. Horizontal movement stabilization (wrist-arc drift suppression &
///    touchpad aspect-ratio compensation) for high-precision horizontal drags.
/// 4. Velocity-adaptive sub-pixel EMA smoothing to eliminate quantization jitter
///    at low speeds while keeping zero latency at high speeds.
#[derive(Debug, Clone)]
pub struct DynamicAccelTracker {
    /// Temporally smoothed finger speed (normalized touchpad units / ms).
    smoothed_velocity: f32,
    /// Previous frame raw finger speed for instantaneous acceleration (dv/dt).
    prev_velocity: f32,
    /// Smoothed movement direction vector for horizontal trajectory stabilization.
    dir_x: f32,
    dir_y: f32,
    /// Previous frame vertical delta sign during horizontal movement (wobble suppression).
    prev_raw_dy: f32,
    /// Sub-pixel EMA filter state for low-speed precision smoothing.
    ema_dx: f32,
    ema_dy: f32,
    has_state: bool,
}

impl Default for DynamicAccelTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicAccelTracker {
    pub fn new() -> Self {
        Self {
            smoothed_velocity: 0.0,
            prev_velocity: 0.0,
            dir_x: 0.0,
            dir_y: 0.0,
            prev_raw_dy: 0.0,
            ema_dx: 0.0,
            ema_dy: 0.0,
            has_state: false,
        }
    }

    /// Reset all velocity and filter states (e.g. on drag start or stop).
    pub fn reset(&mut self) {
        self.smoothed_velocity = 0.0;
        self.prev_velocity = 0.0;
        self.dir_x = 0.0;
        self.dir_y = 0.0;
        self.prev_raw_dy = 0.0;
        self.ema_dx = 0.0;
        self.ema_dy = 0.0;
        self.has_state = false;
    }

    /// Called when fingers are held stationary during an active drag.
    /// Decays velocity state cleanly so resuming movement starts from precision mode.
    pub fn on_stationary(&mut self) {
        self.smoothed_velocity *= 0.35;
        if self.smoothed_velocity < 0.02 {
            self.smoothed_velocity = 0.0;
        }
        self.prev_velocity = 0.0;
        self.prev_raw_dy = 0.0;
        self.ema_dx = 0.0;
        self.ema_dy = 0.0;
        self.has_state = false;
    }

    /// Process a raw consensus touchpad delta `(dx, dy)` (in 0..4096 normalized coordinates)
    /// and return the final screen pixel delta `(out_dx, out_dy)`.
    pub fn process_delta(
        &mut self,
        device_id: &str,
        config: &Config,
        delta: (f32, f32),
        elapsed_ms: i64,
        aspect_ratio: f32,
    ) -> (f32, f32) {
        let device_config = config.device_config(device_id);
        let base_speed = device_config.cursor_speed / 60.0;

        // 1. Compensate for physical touchpad aspect ratio (X span vs Y span).
        // Normalizing rectangular touchpad coordinates independently into 4096x4096
        // compresses horizontal sensitivity relative to vertical sensitivity.
        let horiz_comp = aspect_ratio.clamp(1.0, 1.85).powf(0.60);
        let raw_dx = delta.0 * horiz_comp;
        let mut raw_dy = delta.1;

        let raw_dist = (raw_dx * raw_dx + raw_dy * raw_dy).sqrt();
        if raw_dist < 1e-4 || !raw_dist.is_finite() {
            self.on_stationary();
            return (0.0, 0.0);
        }

        // 2. Horizontal trajectory stabilization ("三指横移精确度优化"):
        // When 3 fingers move side-by-side horizontally, natural wrist pivot and
        // differential finger length introduce slight vertical arc drift.
        let norm_x = raw_dx / raw_dist;
        let norm_y = raw_dy / raw_dist;
        if self.has_state {
            self.dir_x = 0.65 * self.dir_x + 0.35 * norm_x;
            self.dir_y = 0.65 * self.dir_y + 0.35 * norm_y;
        } else {
            self.dir_x = norm_x;
            self.dir_y = norm_y;
        }

        let abs_dx = raw_dx.abs();
        let abs_dy = raw_dy.abs();
        let is_horizontal_dominant = abs_dx > 1.45 * abs_dy && self.dir_x.abs() > 1.35 * self.dir_y.abs();

        if is_horizontal_dominant && abs_dx > 0.2 {
            let ratio = abs_dy / abs_dx;
            // Within ~16.7 degrees of horizontal (tan(16.7°) ≈ 0.30) and moderate vertical delta:
            // apply a smooth C1 cubic-quadratic attenuation to suppress unintentional wrist-arc vertical drift
            // while preserving 100% of intentional diagonal movement once ratio >= 0.30.
            const HORIZ_LOCK_RATIO: f32 = 0.30;
            if ratio < HORIZ_LOCK_RATIO && abs_dy < 9.0 {
                let t = (ratio / HORIZ_LOCK_RATIO).clamp(0.0, 1.0);
                let mut vert_scale = t * t * (0.12 + 0.88 * t);

                // Extra suppression if vertical delta is oscillating (+dy / -dy wobble) across frames
                if self.prev_raw_dy != 0.0 && (raw_dy * self.prev_raw_dy) < 0.0 && abs_dy < 4.5 {
                    vert_scale *= 0.40;
                }
                raw_dy *= vert_scale;
            }
        }
        self.prev_raw_dy = delta.1;

        // 3. Dynamic velocity envelope with frame-interval jitter clamping.
        // Clamp elapsed_ms to [4, 50]ms so coalesced WM_INPUT messages (1-2ms)
        // never create artificial velocity spikes.
        let dt = if elapsed_ms > 0 {
            (elapsed_ms as f32).clamp(4.0, 50.0)
        } else {
            10.0
        };

        let inst_velocity = (raw_dist / dt).min(16.0);
        let accel_boost = if self.has_state && inst_velocity > self.smoothed_velocity {
            // Fast attack when accelerating + instantaneous jerk boost for responsive flicks
            let dv = (inst_velocity - self.prev_velocity).max(0.0);
            self.smoothed_velocity += 0.62 * (inst_velocity - self.smoothed_velocity);
            (dv * 0.24).min(1.25)
        } else if self.has_state {
            // Smooth decay when cruising or decelerating into a precise stop
            self.smoothed_velocity += 0.30 * (inst_velocity - self.smoothed_velocity);
            0.0
        } else {
            self.smoothed_velocity = inst_velocity;
            0.0
        };
        self.prev_velocity = inst_velocity;

        let effective_velocity = (self.smoothed_velocity + accel_boost).clamp(0.0, 16.0);

        // 4. Compute dynamic acceleration multiplier from the 3-stage curve.
        let a = (device_config.cursor_acceleration / 10.0).max(0.0);
        let pointer_gain = compute_dynamic_gain(effective_velocity, a);

        let mut dx = raw_dx * base_speed * pointer_gain;
        let mut dy = raw_dy * base_speed * pointer_gain;

        // 5. Velocity-adaptive sub-pixel trajectory smoothing:
        // Smooths out ADC quantization steps at low speed while keeping high-speed lag at zero.
        if self.has_state {
            // Blend factor ramps from 0.58 at very slow speed to 1.0 (no lag) at v >= 1.5
            let speed_t = ((effective_velocity - 0.15) / 1.35).clamp(0.0, 1.0);
            let alpha_x = 0.62 + 0.38 * speed_t;
            let alpha_y = if is_horizontal_dominant {
                // Stronger vertical smoothing during horizontal drags for rock-steady horizontal tracking
                0.42 + 0.58 * speed_t
            } else {
                alpha_x
            };
            dx = alpha_x * dx + (1.0 - alpha_x) * self.ema_dx;
            dy = alpha_y * dy + (1.0 - alpha_y) * self.ema_dy;
        }

        self.ema_dx = dx;
        self.ema_dy = dy;
        self.has_state = true;

        (dx, dy)
    }
}

/// Compute the 3-stage dynamic acceleration multiplier.
///
/// - `velocity`: effective finger velocity in normalized touchpad units per ms.
/// - `a`: normalized acceleration setting (`cursor_acceleration / 10.0`, default ~1.0–1.2).
pub fn compute_dynamic_gain(velocity: f32, a: f32) -> f32 {
    if a <= 0.001 || !velocity.is_finite() {
        return 1.0;
    }

    let v = velocity.max(0.0) as f64;
    let a64 = a as f64;

    // Stage 1: Low-speed precision floor (slow movements scale down to ~0.46x-0.52x for pixel precision)
    let min_gain = (0.68 - 0.16 * a64.min(2.2)).clamp(0.32, 0.70);

    // Stage 2: Mid-speed ergonomic S-curve transition (centered around v = 1.15 units/ms)
    let mid_span = 0.55 + 0.42 * a64.powf(0.75);
    let steepness = 2.15 * a64.sqrt().clamp(0.5, 2.0);
    let s_curve = sigmoid(steepness * (v - 1.15));
    let base_gain = min_gain + mid_span * s_curve;

    // Stage 3: High-speed progressive logarithmic/power boost for fast cross-screen drags
    let high_v = (v - 1.75).max(0.0);
    let high_boost = if high_v > 0.0 {
        0.42 * a64.powf(0.80) * (1.0 + high_v * 1.15).ln()
    } else {
        0.0
    };

    (base_gain + high_boost).clamp(min_gain, 3.6) as f32
}

/// Apply speed multiplier and dynamic acceleration curve to a delta vector (stateless).
/// Returns (dx, dy) with speed and acceleration applied.
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

    let raw_dist = (delta.0 * delta.0 + delta.1 * delta.1).sqrt();
    let dt = if elapsed_ms > 0 {
        (elapsed_ms as f32).clamp(4.0, 50.0)
    } else {
        10.0
    };
    let mut finger_velocity = (raw_dist / dt).min(16.0);
    if finger_velocity.is_nan() || finger_velocity.is_infinite() {
        finger_velocity = 1.0;
    }

    let a = (device_config.cursor_acceleration / 10.0).max(0.0);
    let pointer_velocity = compute_dynamic_gain(finger_velocity, a);

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
    fn test_dynamic_accel_progressive_curve() {
        let mut config = Config::default();
        let mut dc = DeviceDragConfig::default();
        dc.cursor_speed = 60.0;
        dc.cursor_acceleration = 12.0;
        config.device_configs.insert("test".into(), dc);

        // Verify gain increases monotonically from slow precision -> normal -> fast -> flick
        let a = 1.2;
        let g_slow = compute_dynamic_gain(0.15, a);
        let g_normal = compute_dynamic_gain(1.2, a);
        let g_fast = compute_dynamic_gain(3.5, a);
        let g_flick = compute_dynamic_gain(8.0, a);

        assert!(g_slow < 0.70, "slow gain {} should offer high precision (<0.70)", g_slow);
        assert!(g_normal > g_slow && g_normal > 0.90);
        assert!(g_fast > g_normal * 1.35);
        assert!(g_flick > g_fast && g_flick > 2.0);
    }

    #[test]
    fn test_horizontal_stabilization_suppresses_vertical_drift() {
        let mut config = Config::default();
        let mut dc = DeviceDragConfig::default();
        dc.cursor_speed = 60.0;
        dc.cursor_acceleration = 10.0;
        config.device_configs.insert("test".into(), dc);

        let mut tracker = DynamicAccelTracker::new();
        // Simulate predominantly horizontal movement with small wrist-arc vertical drift (dx=20, dy=1.5)
        let _ = tracker.process_delta("test", &config, (20.0, 1.5), 10, 1.5);
        let (dx, dy) = tracker.process_delta("test", &config, (20.0, 1.5), 10, 1.5);

        assert!(dx > 15.0);
        // Vertical drift ratio dy/dx was 0.075 originally; after stabilization it should be heavily attenuated
        assert!((dy / dx).abs() < 0.015, "vertical drift ratio {} should be strongly attenuated", dy / dx);

        // Intentional diagonal movement (dx=20, dy=15) should NOT be suppressed
        let mut diag_tracker = DynamicAccelTracker::new();
        let _ = diag_tracker.process_delta("test", &config, (20.0, 15.0), 10, 1.0);
        let (ddx, ddy) = diag_tracker.process_delta("test", &config, (20.0, 15.0), 10, 1.0);
        assert!((ddy / ddx - 0.75).abs() < 0.05);
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