//ricostynha author
# Copyright (c) 2024 alvaro ricostynha. All rights reserved.
#
# This file is part of the project. Unauthorized copying, distribution,
# or use of this file, via any medium, is strictly prohibited.

#!/usr/bin/env python3
"""Program to count lines in files and append the count to each file."""

def count_and_append(filename):
    with open(filename, 'r') as f:
        lines = f.readlines()
    line_count = len(lines)
    
    with open(filename, 'a') as f:
        f.write(f" {line_count} lines\n")
    
    print(f"{filename}: {line_count} lines")

# Count and append to both files
count_and_append('run_test.py')

# Description: Counts lines in project files and appends count to each file.
# Author: alvaro ricostynha
# Purpose: Utility script for project line counting

hello I am an AI