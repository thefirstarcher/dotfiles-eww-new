#!/usr/bin/env python3
import os
import re
import sys

# Configuration
STYLES_DIR = os.path.expanduser("~/.config/eww/styles")
THEME_PREFIX = "_theme-"
IGNORE_FILES = ["theme-config.scss", "_modifiers.scss", "_reset.scss"]

# Regex patterns
VAR_DEF_PATTERN = re.compile(r'^\s*\$([a-zA-Z0-9_-]+)\s*:')
VAR_USE_PATTERN = re.compile(r'\$([a-zA-Z0-9_-]+)')

class SCSSFile:
    def __init__(self, path):
        self.path = path
        self.name = os.path.basename(path)
        self.defined = set()
        self.used = set()
        self.parse()

    def parse(self):
        with open(self.path, 'r') as f:
            lines = f.readlines()

        for line in lines:
            # Strip comments
            line = line.split('//')[0].strip()
            if not line:
                continue

            # Check for definitions
            def_match = VAR_DEF_PATTERN.match(line)
            if def_match:
                self.defined.add(def_match.group(1))

            # Check for usages (excluding definitions on the same line)
            # We remove the definition part to search for usages in the value
            value_part = line
            if ':' in line:
                value_part = line.split(':', 1)[1]

            for use_match in VAR_USE_PATTERN.finditer(value_part):
                self.used.add(use_match.group(1))

def main():
    if not os.path.exists(STYLES_DIR):
        print(f"Error: Styles directory not found at {STYLES_DIR}")
        sys.exit(1)

    theme_files = []
    component_files = []

    # 1. Categorize files
    for f in os.listdir(STYLES_DIR):
        if not f.endswith(".scss"):
            continue
        if f in IGNORE_FILES:
            continue

        full_path = os.path.join(STYLES_DIR, f)
        if f.startswith(THEME_PREFIX):
            theme_files.append(SCSSFile(full_path))
        else:
            component_files.append(SCSSFile(full_path))

    print(f"Found {len(theme_files)} themes and {len(component_files)} components.\n")

    # 2. Build set of ALL required variables from components
    # We exclude variables that are defined locally within the same component
    required_vars = set()

    for comp in component_files:
        # Variables used but not defined locally
        needed = comp.used - comp.defined
        required_vars.update(needed)

    # 3. Check each theme
    all_good = True

    for theme in theme_files:
        missing = required_vars - theme.defined
        # Ignore specific built-ins or false positives if necessary
        missing = {v for v in missing if not v.startswith('eww')}

        if missing:
            all_good = False
            print(f"❌ THEME ERROR: {theme.name}")
            print(f"   Missing {len(missing)} variables:")
            for v in sorted(missing):
                # Find which component uses it for context
                users = [c.name for c in component_files if v in c.used]
                print(f"     - ${v} (used in: {', '.join(users)})")
            print("")
        else:
            print(f"✅ {theme.name} is complete.")

    if all_good:
        print("\n🎉 All themes contain all required variables!")
        sys.exit(0)
    else:
        print("\n⚠️  Fix missing variables in the themes listed above.")
        sys.exit(1)

if __name__ == "__main__":
    main()
