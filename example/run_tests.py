#!/usr/bin/env python3
"""
Script to run all tests for the bubble sort implementation.
"""
# Pois
import subprocess
import sys

def fibonacci(n: int) -> int:
    """Compute the nth Fibonacci number."""
    if n <= 0:
        return 0
    elif n == 1:
        return 1
    else:
        return fibonacci(n - 1) + fibonacci(n - 2)

def run_tests():
    """Run all test suites."""
    print("Running bubble sort tests...")
    print("=" * 40)
    
    # Run the built-in tests first
    print("1. Running built-in tests from cenas.py:")
    try:
        result = subprocess.run([sys.executable, "cenas.py"], 
                              capture_output=True, text=True)
        if result.returncode == 0:
            print("   Built-in tests: PASSED")
        else:
            print("   Built-in tests: FAILED")
            print(result.stdout)
            print(result.stderr)
    except Exception as e:
        print(f"   Error running built-in tests: {e}")
    
    print("\n2. Running unit tests from test_cenas.py:")
    try:
        result = subprocess.run([sys.executable, "-m", "unittest", "test_cenas.py", "-v"], 
                              capture_output=True, text=True)
        if result.returncode == 0:
            print("   Unit tests: PASSED")
        else:
            print("   Unit tests: FAILED")
            print(result.stdout)
            print(result.stderr)
    except Exception as e:
        print(f"   Error running unit tests: {e}")
    
    print("\n" + "=" * 40)
    print("Test execution completed.")

if __name__ == "__main__":
    run_tests()

Oi vize

cenas

# Random cenas d epython to remo