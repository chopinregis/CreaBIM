"""Check model description files against docs/creabim-model.schema.json.

An independent check of the schema CreaBIM writes for its JSON model format
(docs/json-format.md): the files CreaBIM builds must also be valid to a
standard JSON Schema validator.

    python tools/validate_model_json.py examples/small-house.json [more.json ...]
"""

import json
import sys
from pathlib import Path

import jsonschema

ROOT = Path(__file__).resolve().parent.parent


def main(paths):
    schema = json.loads((ROOT / "docs" / "creabim-model.schema.json").read_text())
    jsonschema.Draft202012Validator.check_schema(schema)
    validator = jsonschema.Draft202012Validator(schema)
    bad = 0
    for p in paths:
        doc = json.loads(Path(p).read_text())
        errors = sorted(validator.iter_errors(doc), key=lambda e: list(e.absolute_path))
        for e in errors:
            where = "$" + "".join(
                f"[{k}]" if isinstance(k, int) else f".{k}" for k in e.absolute_path
            )
            print(f"{p}: {where}: {e.message}")
        bad += len(errors)
        if not errors:
            print(f"{p}: valid")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
