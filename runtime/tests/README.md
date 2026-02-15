# Pipit Runtime Library Tests

Unit tests for the Pipit standard actor library (`std_actors.h`).

## Overview

These tests validate the runtime behavior of individual actors:

- **test_arithmetic.cpp**: Arithmetic operators (mul, add, sub, div, abs, sqrt, threshold)
- **test_statistics.cpp**: Statistical functions (mean, rms, min, max)
- **test_fft.cpp**: FFT implementation (Cooley-Tukey algorithm validation)

## Building and Running

```bash
cd runtime/tests
cmake . -B build
cd build
make
ctest --verbose
```

## Test Categories

### Arithmetic Tests

- Basic operations (mul, add, sub, div)
- Edge cases (division by zero → NaN, sqrt of negative → NaN)
- Absolute value and square root
- Threshold detector (float → int32 conversion)

### Statistics Tests

- Window-based statistics (mean, rms, min, max)
- Various window sizes (N=1, 4, 5, 100)
- Edge cases (negative values, all same values, single value)
- Large windows (N=100)

### FFT Tests

- Power-of-2 validation (non-power-of-2 returns ACTOR_ERROR)
- DC signal (verifies bin 0 magnitude)
- Impulse response (flat spectrum)
- Cosine wave (frequency bin localization)
- Parseval's theorem (energy preservation)
- Linearity property
- Large FFT (N=256)

## Adding New Tests

1. Create a new test file (e.g., `test_new_actors.cpp`)
2. Use the TEST() macro for each test case
3. Add to `CMakeLists.txt`:

   ```cmake
   add_executable(test_new_actors test_new_actors.cpp)
   add_test(NAME NewActors COMMAND test_new_actors)
   ```

4. Build and run: `make && ctest`

## Test Macros

- `TEST(name)` - Define a test case
- `ASSERT_EQ(actual, expected)` - Assert equality
- `ASSERT_NEAR(actual, expected, epsilon)` - Assert approximate equality
- `ASSERT_COMPLEX_NEAR(actual, expected, epsilon)` - Assert complex number equality
- `ASSERT_TRUE(condition)` - Assert boolean condition

All assertions print detailed error messages and exit with code 1 on failure.
