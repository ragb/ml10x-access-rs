# ml10x-access-rs

Rust port of [ml10x-access](https://github.com/ragb/ml10x-access) — an
accessible CLI and YAML workflow for the Morningstar ML10X loop switcher.

The Python project at `../ml10x-access` is the reference implementation
and remains authoritative for protocol facts. This crate exists to
produce a single distributable binary with no Python install friction.

Status: **work in progress**. See `../ml10x-access/.claude/plans/wild-brewing-walrus.md`
for the phased plan.

License: GPL-3.0-or-later (inherited from the Python project, which
itself derives from the GPLv3-licensed `morningstarmidi` reverse
engineering library).
