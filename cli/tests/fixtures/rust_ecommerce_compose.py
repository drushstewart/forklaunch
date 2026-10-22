"""Validate generated worker ownership, isolation and user settings across CLI lifecycle."""
import json
from pathlib import Path
import re
import subprocess
import sys

mode = sys.argv[1]
compose = json.loads(subprocess.check_output([
    'docker', 'compose', 'config', '--no-interpolate', '--format', 'json'
]))
services = compose['services']
assert 'ecommerce-inventory-worker' in services
if mode == 'deleted':
    assert 'renamed-shop-inventory-worker' not in services
    assert 'spare-shop-inventory-worker' not in services
else:
    name = 'renamed-shop' if mode == 'renamed' else 'spare-shop'
    worker = services[f'{name}-inventory-worker']
    assert worker['command'] == [f'/app/bin/{name}', '--worker']
    assert worker['x-forklaunch-ecommerce-worker-for'] == name
    assert worker['image'] == services[name]['image']
    assert not worker.get('ports')
    env = worker['environment']
    for key in ['DATABASE_URL', 'TENANT_ID', 'ENCRYPTION_KEY']:
        assert env[key] == services[name]['environment'][key]
    assert not any(key.startswith(('STRIPE_', 'PAYPAL_')) or key in ['HMAC_SECRET_KEY', 'JWKS_PUBLIC_KEY_URL'] for key in env)
    assert env['WORKER_HTTP_ADDR'] == '0.0.0.0:9090'
    assert worker['healthcheck']['test'][-1] == 'http://localhost:9090/ready'
    if mode == 'prepare':
        source = Path('docker-compose.yaml').read_text()
        section = re.search(r'(?ms)^  spare-shop-inventory-worker:\n.*?(?=^  \S|\Z)', source)
        assert section
        updated = section[0].replace('  spare-shop-inventory-worker:\n', '  spare-shop-inventory-worker:\n    x-user-worker-marker: preserved\n', 1).replace("      WORKER_BATCH_SIZE: '1'\n", '      WORKER_BATCH_SIZE: "2"\n', 1)
        Path('docker-compose.yaml').write_text(source[:section.start()] + updated + source[section.end():])
    else:
        assert worker['x-user-worker-marker'] == 'preserved'
        assert env['WORKER_BATCH_SIZE'] == '2'
        if mode == 'renamed':
            assert 'spare-shop-inventory-worker' not in services
print(f'PASS generated ecommerce worker Compose {mode}: owner, shared image/storage, credential isolation, readiness and preserved overrides')
