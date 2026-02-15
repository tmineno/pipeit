#!/usr/bin/env python3
"""
Generate binary test data files for binread/binwrite actor tests.

Creates test files with known sequences for validation:
- test_int16.bin: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as int16
- test_int32.bin: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as int32
- test_float.bin: [1.0, 2.0, ..., 10.0] as float32
- test_cfloat.bin: [(1+0j), (2+0j), ..., (10+0j)] as complex64
"""

import struct
import os

def main():
    # Get the directory where this script is located
    script_dir = os.path.dirname(os.path.abspath(__file__))

    # test_int16.bin: 10 int16 values [1, 2, 3, ..., 10]
    with open(os.path.join(script_dir, 'test_int16.bin'), 'wb') as f:
        for i in range(1, 11):
            f.write(struct.pack('<h', i))
    print("Created test_int16.bin: 10 int16 values [1..10]")

    # test_int32.bin: 10 int32 values [1, 2, 3, ..., 10]
    with open(os.path.join(script_dir, 'test_int32.bin'), 'wb') as f:
        for i in range(1, 11):
            f.write(struct.pack('<i', i))
    print("Created test_int32.bin: 10 int32 values [1..10]")

    # test_float.bin: 10 float32 values [1.0, 2.0, ..., 10.0]
    with open(os.path.join(script_dir, 'test_float.bin'), 'wb') as f:
        for i in range(1, 11):
            f.write(struct.pack('<f', float(i)))
    print("Created test_float.bin: 10 float32 values [1.0..10.0]")

    # test_cfloat.bin: 10 complex64 values [(1+0j), (2+0j), ..., (10+0j)]
    with open(os.path.join(script_dir, 'test_cfloat.bin'), 'wb') as f:
        for i in range(1, 11):
            # complex64 = two float32 (real, imag)
            f.write(struct.pack('<ff', float(i), 0.0))
    print("Created test_cfloat.bin: 10 complex64 values [(1+0j)..(10+0j)]")

    print("\nAll test data files created successfully!")
    print(f"Location: {script_dir}")

if __name__ == '__main__':
    main()
