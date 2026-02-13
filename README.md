# Pipit

> **Status: Early stage — Work in Progress**

Pipit is a domain-specific language for describing clock-driven, real-time data pipelines using Synchronous Dataflow (SDF) semantics on shared memory.

## What it does

- Define actors in C++ with static input/output token rates
- Describe pipelines in `.pdl` files with a concise pipe-based syntax
- Compile to native executables via C++ code generation

```text
source.pdl → pcc → source_gen.cpp → g++/clang++ → executable
```

## Example

```text
const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]

clock 10MHz capture {
    adc(0) | fft(256) | :raw | fir(coeff) -> signal
    :raw | mag() | stdout()
}

clock 1kHz drain {
    @signal | decimate(10000) | csvwrite("output.csv")
}
```

## Documentation

- [Language Spec v0.1.0](doc/spec/pipit-lang-spec-v0.1.0.md)
- [Development TODO](doc/TODO.md)
