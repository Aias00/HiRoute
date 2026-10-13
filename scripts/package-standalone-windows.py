#!/usr/bin/env python3
"""Windows entry to the single standalone-package producer (Python 3.9+)."""
import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location("hiroute_standalone_package", Path(__file__).with_name("package-standalone.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)

if __name__ == "__main__":
    package.main()
