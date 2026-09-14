#!/usr/bin/env python3
"""Build a directly executable observer with an explicitly pinned Python runtime."""
import argparse
from pathlib import Path
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', required=True, type=Path)
args = parser.parse_args()
interpreter = str(Path(sys.executable).resolve())
if any(character.isspace() for character in interpreter) or len(interpreter.encode()) > 120:
    parser.error('Python interpreter must have a short absolute path without whitespace')
with args.output.open('xb') as output:
    output.write(('#!' + interpreter + '\n').encode())
    output.write(Path(__file__).with_name('observer.py').read_bytes())
args.output.chmod(0o700)
