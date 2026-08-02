# v0.5 Compatibility Mode

Rad keeps v0.4 as the stable baseline. Selected v0.5 ergonomics are available through an opt-in flag.

## Enable it

```bash
rad file.rad --compat-v0.5-dx
```

## What it adds

**Zero-field variant shorthand** — omit the braces:

```
// Without compat:
AccessSignal::MfaDisabled { }

// With compat:
AccessSignal::MfaDisabled
```

**Match rest binding** — ignore unneeded fields with `..`:

```
match sig {
    SuspiciousGeo { region, .. } => { print(region) }
    MfaDisabled => { print("off") }
}
```

Rules: `..` can appear at most once per arm, must be the final entry.

## Warning flags

| Flag | Effect |
|---|---|
| `--warn-compat` | Enable compatibility warnings (default) |
| `--no-warn-compat` | Suppress compatibility warnings |
| `--deny-warnings` | Treat warnings as errors |

## Diagnostics

| Code | Meaning |
|---|---|
| `E2501` | Ambiguous qualified reference matches both sum variant and state |
| `E2502` | Shorthand used without `--compat-v0.5-dx`, or used for a variant that requires fields |
| `E2503` | Invalid rest binding without `--compat-v0.5-dx` |
| `E2504` | Unknown binding name in a match pattern |
| `W2501` | Disambiguation warning for overlapping names |
