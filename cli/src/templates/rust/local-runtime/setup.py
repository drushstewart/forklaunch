#!/usr/bin/env python3
"""Create local credentials once; never overwrite an existing environment."""
from pathlib import Path
import os
import secrets

root = Path(__file__).resolve().parents[1]
values = {name: secrets.token_hex(32) for name in (
    'POSTGRES_PASSWORD', 'API_DB_PASSWORD', 'AUTH_DB_PASSWORD',
    'WORKER_DB_PASSWORD', 'INTERNAL_AUTH_SECRET', 'BETTER_AUTH_SECRET',
    'ENCRYPTION_KEY_HEX', 'DEMO_PASSWORD', 'OBJECT_STORE_PASSWORD', 'REDIS_PASSWORD',
)}
values['PUBLIC_ORIGIN'] = 'https://localhost:@@PORT@@'
try:
    descriptor = os.open(root / '.env', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
except FileExistsError:
    print('Keeping existing .env; no credentials changed.')
    raise SystemExit(0)
with os.fdopen(descriptor, 'w') as output:
    output.write(''.join(f'{key}={value}\n' for key, value in values.items()))
print('Created private .env; credentials were not printed.')
