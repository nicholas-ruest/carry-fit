# carry-fit

`carry-fit` finds the highest-value packing list that actually fits. Give it a bag, candidate items, hard requirements, dependencies, and mutually exclusive choices; it performs an exact search and prints a ready-to-use checklist.

The optimizer uses integer grams and millilitres, so results are reproducible across machines. Ties prefer lower weight, then lower volume, then lexicographically smaller item IDs.

## Install

Stable Rust 1.85 or newer is required.

```sh
cargo install --path .
```

## Try it

```sh
carry-fit examples/weekend.toml
carry-fit examples/weekend.toml --format json
carry-fit examples/weekend.toml --format markdown --output packing-list.md
```

Example terminal output:

```text
Weekend carry-on
================
[x] Two-day clothing set (chosen, 1600 g, 4800 ml, score 85)
...
9 items | score 491 | 4100 g | 11840 ml
remaining: 100 g and 160 ml
```

Existing output files are protected by default. Pass `--force` with `--output` to replace one intentionally.

## Manifest format

```toml
[bag]
name = "Overnight bag"
max_weight_grams = 3000
max_volume_ml = 10000

[[item]]
id = "passport"
name = "Passport"
weight_grams = 40
volume_ml = 30
score = 100
required = true

[[item]]
id = "camera"
name = "Camera"
weight_grams = 600
volume_ml = 1200
score = 35
needs = ["camera-battery"]

[[item]]
id = "rain-shell"
name = "Rain shell"
weight_grams = 340
volume_ml = 1500
score = 45
group = "outerwear"
```

- `score` is the item's additive personal value and must be positive.
- `required = true` forces the item and the transitive closure of its dependencies.
- `needs` lists item IDs that must also be selected.
- `group` allows at most one selected item with the same group, such as `outerwear` or `spare-footwear`.
- Unknown fields, duplicate IDs, missing dependencies, cycles, and impossible required loads fail closed.

## Exactness and limits

The search is exact, not heuristic. To keep runtime bounded and predictable, a manifest may contain at most 24 candidate items. The current model optimizes one bag, two capacities, additive scores, dependencies, and at-most-one groups. It does not model item rotation, compressibility, multiple bags, or non-additive preferences.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

The crate forbids `unsafe` code. CI runs the same checks on stable Rust and checks the declared Rust 1.85 minimum supported version.

## License

MIT
