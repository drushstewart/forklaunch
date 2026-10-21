#!/usr/bin/env python3
"""All retained resource dialects, with one shared-build lock per complete case."""
import json
import os
from pathlib import Path
import subprocess
import tempfile


MODES = [
    'static', 'legacy', 'mixed', 'dynamic', 'recursive',
    'fragment4', 'fragment6', 'fragment7', 'siblings', 'composed',
    'uri4', 'uri6', 'uri7', 'pointer4', 'pointer6', 'pointer7',
]
script = Path(__file__).with_name('rust_recursive_sdk_runtime.sh')
results = []
with tempfile.TemporaryDirectory(prefix='forklaunch-schema-resource-receipts-') as directory:
    for mode in MODES:
        receipt = Path(directory) / (mode + '.json')
        environment = {
            **os.environ,
            'FORKLAUNCH_SCHEMA_RESOURCE_MODES': mode,
            'FORKLAUNCH_RECURSIVE_SDK_RECEIPT': str(receipt),
        }
        print(f'Running complete schema resource matrix: {mode}', flush=True)
        result = subprocess.run(['bash', str(script)], env=environment, check=False)
        evidence = json.loads(receipt.read_text()) if receipt.is_file() else None
        results.append({
            'mode': mode,
            'exitCode': result.returncode,
            'passed': result.returncode == 0 and evidence is not None
                      and evidence.get('status') == 'PASS'
                      and evidence.get('schemaResourceModes') == [mode],
            'receipt': evidence,
        })

hashes = {result['receipt']['cliSha256'] for result in results if result['receipt']}
passed = len(results) == len(MODES) and all(result['passed'] for result in results) and len(hashes) == 1
aggregate = {
    'status': 'PASS' if passed else 'FAIL',
    'schemaResourceModes': MODES,
    'cliSha256': next(iter(hashes)) if len(hashes) == 1 else None,
    'singleCapture': len(hashes) == 1,
    'cases': results,
}
destination = os.environ.get('FORKLAUNCH_RECURSIVE_SDK_RECEIPT')
if destination:
    Path(destination).write_text(json.dumps(aggregate, indent=2) + '\n')
print(json.dumps(aggregate, indent=2), flush=True)
raise SystemExit(0 if passed else 1)
