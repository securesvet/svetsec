---
labels:
  - animation
  - mathematics
---

# Animation

A dense field of dots bends under two invisible balls following different,
smooth pseudo-random paths. Each terminal character uses a 2×4 braille
subgrid, so dots can slide and accumulate more smoothly. Where the sheet is
compressed, neighboring dots merge into heavier shapes.

```dot-well
```

Each deformation is a radially symmetric Gaussian depression, and the two
fields are added together:

`h(x, y, t) = -Σᵢ Aᵢ(t) exp(-rᵢ² / (2σᵢ²))`

This is a stylized Gaussian height field, not a physical simulation of
gravity. The familiar general-relativity rubber-sheet picture is related to
Flamm's paraboloid.
