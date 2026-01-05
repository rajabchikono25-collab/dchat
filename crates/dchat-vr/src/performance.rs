//! Performance Optimization for VR/AR
//!
//! Optimizations to achieve 90fps+ in VR with minimal latency

use crate::Vector3;

/// Performance metrics
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub frame_time_ms: f32,
    pub fps: f32,
    pub cpu_time_ms: f32,
    pub gpu_time_ms: f32,
    pub visible_avatars: usize,
    pub active_audio_sources: usize,
    pub culled_objects: usize,
}

/// Level of detail settings
#[derive(Debug, Clone, Copy)]
pub enum LodLevel {
    High,   // Full quality, close range
    Medium, // Reduced quality, mid range
    Low,    // Minimal quality, far range
}

impl LodLevel {
    pub fn from_distance(distance: f32) -> Self {
        if distance < 5.0 {
            Self::High
        } else if distance < 15.0 {
            Self::Medium
        } else {
            Self::Low
        }
    }

    pub fn audio_quality(&self) -> f32 {
        match self {
            Self::High => 1.0,
            Self::Medium => 0.7,
            Self::Low => 0.4,
        }
    }

    pub fn poly_count_multiplier(&self) -> f32 {
        match self {
            Self::High => 1.0,
            Self::Medium => 0.4,
            Self::Low => 0.1,
        }
    }
}

/// Frustum for view culling
#[derive(Debug, Clone)]
pub struct Frustum {
    pub position: Vector3,
    pub forward: Vector3,
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

impl Frustum {
    pub fn new(position: Vector3, forward: Vector3, fov: f32) -> Self {
        Self {
            position,
            forward,
            fov,
            near: 0.1,
            far: 100.0,
        }
    }

    /// Check if position is within frustum
    pub fn contains(&self, position: &Vector3) -> bool {
        let to_point = Vector3 {
            x: position.x - self.position.x,
            y: position.y - self.position.y,
            z: position.z - self.position.z,
        };

        let distance = to_point.distance(&Vector3::zero());

        // Check near/far planes
        if distance < self.near || distance > self.far {
            return false;
        }

        // Check angle from forward vector
        let dot = (to_point.x * self.forward.x
            + to_point.y * self.forward.y
            + to_point.z * self.forward.z)
            / distance;
        let angle = dot.acos();

        angle < (self.fov / 2.0)
    }
}

/// Performance optimizer
pub struct PerformanceOptimizer {
    target_fps: f32,
    frame_times: Vec<f32>,
    max_frame_history: usize,
    adaptive_quality: bool,
    culling_enabled: bool,
}

impl PerformanceOptimizer {
    pub fn new(target_fps: f32) -> Self {
        Self {
            target_fps,
            frame_times: Vec::new(),
            max_frame_history: 60, // 1 second at 60fps
            adaptive_quality: true,
            culling_enabled: true,
        }
    }

    /// Record frame time
    pub fn record_frame(&mut self, frame_time_ms: f32) {
        self.frame_times.push(frame_time_ms);
        if self.frame_times.len() > self.max_frame_history {
            self.frame_times.remove(0);
        }
    }

    /// Get current performance metrics
    pub fn get_metrics(&self) -> PerformanceMetrics {
        let avg_frame_time = if !self.frame_times.is_empty() {
            self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32
        } else {
            0.0
        };

        let fps = if avg_frame_time > 0.0 {
            1000.0 / avg_frame_time
        } else {
            0.0
        };

        PerformanceMetrics {
            frame_time_ms: avg_frame_time,
            fps,
            cpu_time_ms: avg_frame_time * 0.6, // Estimate
            gpu_time_ms: avg_frame_time * 0.4, // Estimate
            visible_avatars: 0,
            active_audio_sources: 0,
            culled_objects: 0,
        }
    }

    /// Check if performance is below target
    pub fn needs_optimization(&self) -> bool {
        let metrics = self.get_metrics();
        metrics.fps < self.target_fps
    }

    /// Get recommended LOD level based on performance
    pub fn get_recommended_lod(&self) -> LodLevel {
        if !self.adaptive_quality {
            return LodLevel::High;
        }

        let metrics = self.get_metrics();

        if metrics.fps < self.target_fps * 0.8 {
            LodLevel::Low
        } else if metrics.fps < self.target_fps * 0.95 {
            LodLevel::Medium
        } else {
            LodLevel::High
        }
    }

    /// Cull objects outside frustum
    pub fn cull_objects(&self, frustum: &Frustum, positions: &[Vector3]) -> Vec<usize> {
        if !self.culling_enabled {
            return (0..positions.len()).collect();
        }

        positions
            .iter()
            .enumerate()
            .filter(|(_, pos)| frustum.contains(pos))
            .map(|(i, _)| i)
            .collect()
    }

    /// Set adaptive quality
    pub fn set_adaptive_quality(&mut self, enabled: bool) {
        self.adaptive_quality = enabled;
    }

    /// Set frustum culling
    pub fn set_culling(&mut self, enabled: bool) {
        self.culling_enabled = enabled;
    }
}

impl Default for PerformanceOptimizer {
    fn default() -> Self {
        Self::new(90.0) // Target 90fps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lod_from_distance() {
        assert!(matches!(LodLevel::from_distance(3.0), LodLevel::High));
        assert!(matches!(LodLevel::from_distance(10.0), LodLevel::Medium));
        assert!(matches!(LodLevel::from_distance(20.0), LodLevel::Low));
    }

    #[test]
    fn test_frustum_culling() {
        let frustum = Frustum::new(
            Vector3::zero(),
            Vector3::new(0.0, 0.0, 1.0),
            90.0f32.to_radians(),
        );

        // Point in front should be visible
        assert!(frustum.contains(&Vector3::new(0.0, 0.0, 5.0)));

        // Point behind should be culled
        assert!(!frustum.contains(&Vector3::new(0.0, 0.0, -5.0)));

        // Point too far should be culled
        assert!(!frustum.contains(&Vector3::new(0.0, 0.0, 150.0)));
    }

    #[test]
    fn test_performance_tracking() {
        let mut optimizer = PerformanceOptimizer::new(90.0);

        // Record good performance
        for _ in 0..10 {
            optimizer.record_frame(11.1); // 90fps
        }

        let metrics = optimizer.get_metrics();
        assert!((metrics.fps - 90.0).abs() < 1.0);
        assert!(!optimizer.needs_optimization());
    }

    #[test]
    fn test_adaptive_lod() {
        let mut optimizer = PerformanceOptimizer::new(90.0);

        // Record poor performance
        for _ in 0..10 {
            optimizer.record_frame(20.0); // 50fps
        }

        assert!(optimizer.needs_optimization());
        assert!(matches!(optimizer.get_recommended_lod(), LodLevel::Low));
    }
}
