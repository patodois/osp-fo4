# Photo comparison metrics

These are original diagnostic algorithms, not claims about the game rasterizer.
Cameras are reconstructed from measured words. There is no registration,
exposure fit, geometry fit or parameter search against photographs.

Engine-to-photo edges use visible instance/depth/triangle reciprocal-depth planes.
A neighboring depth is compared with the continuation of the current triangle:
absolute tolerance 0.25 plus relative tolerance 1e-5. Background transitions are
edges. Photo gradient candidates use the published silhouette Options defaults.
Report count, median and p95, retaining unavailable cases. Per-instance statistics
keep source identities; they do not imply a physical cause for every unmatched edge.

Photo-to-engine selection uses two original photographs, never the engine image.
Each RGB8 channel receives a separable 17-tap binomial C(16,k)/65536 blur (sigma 2).
Central gradients select the strongest channel, threshold 4 RGB8 levels/pixel,
bilinear nonmaximum suppression along the normal at +/-1 pixel. Primary edges
persist only with a secondary edge within 1 pixel. The mask cuts pixels; blur
weights normalize over included samples only. Persistent edges farther than
2 pixels from the engine silhouette are missing candidates. Regions connect
across gaps up to 3 pixels; fewer than 8 pixels are counted separately.

For regional attribution, run the same detector on the repeated engine color
image only as a diagnostic. Majority agreement within 1 pixel suggests existing
texture; otherwise majority over a rendered surface suggests appearance/lighting.
Other regions remain unknown. A projected blocked object's documented envelope
is only a candidate association, never proof of runtime pose or correspondence.
NAVM and unknown bounds receive no invented envelope. The repeated-photograph
selection remains independent of these later labels.

An optional annotated opening check selects photo edges along four sides before
consulting engine distances. Publish tangential coverage and maximum/p95 per side.
Do not confuse a local annotated acceptance check with whole-image equivalence.
