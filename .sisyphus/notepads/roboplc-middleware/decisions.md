## Decisions from roboplc-middleware Implementation

### Task 25 - Latency Trend Monitoring
- Chosen approach: replace static high-latency threshold with per-device 3-sigma anomaly detection.
- Rationale: absolute thresholds do not adapt to device-specific baseline latency, while rolling distribution-based thresholding captures abnormal spikes relative to normal behavior.
- Scope boundary: implementation and tests kept entirely in `src/workers/latency_monitor.rs`; no changes to shared structs or dependencies.
