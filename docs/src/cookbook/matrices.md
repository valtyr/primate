# Matrices and fixed shapes

When you have data with a fixed shape — RGB triples, 3×3 matrices,
fixed-byte buffers — use `array<T, N>`. The arity is part of the
type, so generators emit idiomatic fixed-size containers per target.

## RGB triples

```primate
// constants/colors.prim

type Pixel = array<u32, 3>

Pixel RED   = [255, 0, 0]
Pixel GREEN = [0, 255, 0]
Pixel BLUE  = [0, 0, 255]
```

Generated TypeScript (`generated/constants/colors.ts`):

```typescript
export type Pixel = [number, number, number];

export const red   = [255, 0, 0] as const;
export const green = [0, 255, 0] as const;
export const blue  = [0, 0, 255] as const;
```

In Rust the same `Pixel` becomes `[u32; 3]`; in Python it's
`Tuple[int, int, int]`.

## 3×3 matrices

The compact form fits on one line — the formatter keeps it inline:

```primate
type Mat3 = array<array<u32, 3>, 3>

Mat3 SMALL = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
```

For larger matrices the same shape gets long. Use a **trailing comma**
on the outer literal to opt into multi-line layout:

```primate
Mat3 IDENTITY = [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
]
```

The trailing `,` after the last row tells the formatter "keep this
multi-line, even if it would fit on one line." See
[Values](../language/values.md) for the rule.

Generated TypeScript:

```typescript
export type Mat3 = [
  [number, number, number],
  [number, number, number],
  [number, number, number],
];

export const identity = [
  [1, 0, 0],
  [0, 1, 0],
  [0, 0, 1],
] as const;
```

## Length-mismatch is a hard error

```primate
type V3 = array<u32, 3>

V3 SHORT = [1, 2]      // ✗ length-mismatch: expected 3, got 2
V3 LONG  = [1, 2, 3, 4]   // ✗ length-mismatch
```

The error includes the expected and actual lengths and points at the
literal.

## Wider tables

A matrix-like value can also be a `tuple<...>` of heterogeneous types
when the columns have different shapes:

```primate
type RetrySchedule = tuple<u32, duration, duration>

RetrySchedule DEFAULT = [3, 100ms, 30s]
RetrySchedule AGGR    = [10, 50ms, 5s]
```

Tuples and fixed arrays both compile to fixed-shape containers in the
target languages; the difference is only in whether the elements share
a type.

## Lookup tables

For sparse named lookups, reach for `map`:

```primate
map<string, u32> SERVICE_PORTS = {
    "http":  80,
    "https": 443,
    "ssh":   22,
    "smtp":  25,
}
```

Trailing comma keeps it multi-line; without it, short maps stay
inline.
