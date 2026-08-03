# Dogfood math kernels

This project-local native extension accelerates tight finite-mathematics loops
used by the Frankl and Collatz dogfoods. It deliberately uses RAD's generic
`load_extension()` ABI: none of these algorithms are VM opcodes, builtins, or
language semantics.

Build it before running either project:

```powershell
projects/dogfood/native-math-kernels/build.ps1
```

```sh
projects/dogfood/native-math-kernels/build.sh release
```

The scripts also install a stable extensionless name (and the platform name
needed by Windows' loader). The RAD adapters exchange canonical JSON across
the scalar extension ABI.
This keeps the extension boundary stable and lets independent project tools
validate every result.
