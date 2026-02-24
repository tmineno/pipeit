# Diagnostic Code Registry

Stable diagnostic codes for the Pipit compiler (`pcc`).

## Compatibility Policy

- **Reuse prohibition**: Once assigned, a code must never be reassigned to a different
  semantic meaning. Removed diagnostics retire their code permanently.
- **Deprecation**: To change semantics, allocate a new code and add a deprecation note
  to the old one in `compiler/src/diag.rs::codes`.
- **Test contract**: Tests may assert on `Diagnostic.code` values. Changing a code's
  semantics is a breaking change.
- **Versioning**: Code meanings are versioned with the compiler version.

## Code Ranges

| Range | Phase | Description |
|-------|-------|-------------|
| E0001-E0099 | resolve | Name resolution errors |
| E0100-E0199 | type_infer | Type inference errors |
| E0200-E0299 | lower | Lowering verification (L1-L5) |
| E0300-E0399 | analyze | SDF analysis errors |
| E0400-E0499 | schedule | Scheduling errors |
| E0500-E0599 | graph | Graph construction errors |
| E0600-E0699 | pipeline | Stage certification failures |
| W0001-W0099 | resolve | Name resolution warnings |
| W0300-W0399 | analyze | SDF analysis warnings |
| W0400-W0499 | schedule | Scheduling warnings |

## Assigned Codes

### Resolve (E0001-E0023, W0001-W0002)

| Code | Description |
|------|-------------|
| E0001 | Duplicate const definition |
| E0002 | Duplicate param definition |
| E0003 | Duplicate define definition |
| E0004 | Duplicate task definition |
| E0005 | Cross-namespace name collision |
| E0006 | Tap declared but never consumed in define |
| E0007 | Duplicate mode in task |
| E0008 | Undefined tap reference |
| E0009 | Duplicate tap declaration |
| E0010 | Multiple writers to shared buffer |
| E0011 | Unknown actor or define |
| E0012 | Non-polymorphic actor called with type arguments |
| E0013 | Wrong number of type arguments |
| E0014 | Undefined param reference |
| E0015 | Undefined const reference |
| E0016 | Runtime param used as frame dimension |
| E0017 | Unknown name in shape constraint |
| E0018 | Undefined param in switch source |
| E0019 | Switch references undefined mode |
| E0020 | Mode defined but not listed in switch |
| E0021 | Mode listed multiple times in switch |
| E0022 | Undefined tap as actor input |
| E0023 | Shared buffer has no writer |
| W0001 | Define shadows actor with same name |
| W0002 | Deprecated switch default clause |

### Type Inference (E0100-E0102)

| Code | Description |
|------|-------------|
| E0100 | Unknown type name |
| E0101 | Ambiguous polymorphic call (upstream context available) |
| E0102 | Ambiguous polymorphic call (no upstream context) |

### Lowering (E0200-E0206)

| Code | Description |
|------|-------------|
| E0200 | L1: Type consistency violation at edge |
| E0201 | L2: Unsafe widening chain |
| E0202 | L3: Rate/shape preservation violation |
| E0203 | L4: Polymorphic actor not fully monomorphized |
| E0204 | L4: Polymorphic actor has no concrete instance |
| E0205 | L5: Unresolved input type |
| E0206 | L5: Unresolved output type |

### Analysis (E0300-E0310, W0300)

| Code | Description |
|------|-------------|
| E0300 | Unresolved frame dimension |
| E0301 | Conflicting frame constraint from upstream |
| E0302 | Conflicting dimension (span-derived vs edge-inferred) |
| E0303 | Type mismatch at pipe |
| E0304 | SDF balance equation unsolvable |
| E0305 | Feedback loop with no delay |
| E0306 | Shared buffer rate mismatch |
| E0307 | Shared memory pool exceeded |
| E0308 | Param type mismatch |
| E0309 | Switch param non-int32 default |
| E0310 | Control buffer type mismatch |
| W0300 | Inferred dimension param ordering warning |

### Schedule (E0400, W0400)

| Code | Description |
|------|-------------|
| E0400 | Unresolvable cycle in subgraph |
| W0400 | Unsustainable tick rate |

### Graph (E0500)

| Code | Description |
|------|-------------|
| E0500 | Tap not found in graph |

### Pipeline Certs (E0600-E0603)

| Code | Description |
|------|-------------|
| E0600 | HIR verification failed (H1-H3) |
| E0601 | Lowering verification failed (L1-L5) |
| E0602 | Schedule verification failed (S1-S2) |
| E0603 | LIR verification failed (R1-R2) |
