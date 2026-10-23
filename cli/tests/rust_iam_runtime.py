#!/usr/bin/env python3
"""Current raw IAM/Better Auth scaffolds with actual database and SDK consumers."""
from fixtures.rust_builtin_runtime_support import raw_application, build_public_runtime, typecheck, rust_consumer, PostgresDatabase
import hashlib
import json
import re
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib

repo = Path(sys.argv[1]).resolve()
# Better Auth is a TypeScript module; Rust services use its generated SDK.
if len(sys.argv) > 2 and sys.argv[2] == 'better-auth':
    sys.exit('Rust iam-better-auth was retired; use the TypeScript module')
better_auth = False
variant = 'iam-base'
root = Path(tempfile.mkdtemp(prefix='forklaunch-' + variant + '-runtime-')).resolve()
cli = root / 'forklaunch'
shutil.copy2(os.environ['FORKLAUNCH_CLI'], cli)
digest = hashlib.sha256(cli.read_bytes()).hexdigest()
print('CLI SHA-256:', digest, flush=True)
env = dict(os.environ)
role_prefix = 'fl_auth_gen_' if better_auth else 'fl_gen_'
env['FORKLAUNCH_TEST_ROLE'] = f'{role_prefix}{int(time.time())}_{os.getpid()}'
app = root / 'app'
database = PostgresDatabase(env, 'forklaunch-' + variant)
role_created = False
passed = False


def run(*args, cwd=None, failure=False, extra_env=None):
    print('Running:', shlex.join(map(str, args)), flush=True)
    result = subprocess.run(list(map(str, args)), cwd=cwd or app, env={**env, **(extra_env or {})}, check=False, timeout=1800)
    assert (result.returncode != 0) if failure else (result.returncode == 0), result.returncode


def raw(destination, name, **options):
    raw_application(lambda *args: run(*args, cwd=root), cli, destination, name, **options)


def command(*args, path=None, **options):
    run(cli, *args, '--path', path or app, cwd=root, **options)


try:
    raw(root / 'dry', 'auth-dry', dryrun=True)
    assert not (root / 'dry').exists()
    # Removed application presets must fail before writing a destination.
    command('init', 'application', 'preset-rejected', '--modules', 'iam-base', 'iam-better-auth',
            path=root / 'preset-rejected', failure=True)
    assert not (root / 'preset-rejected').exists()
    raw(root / 'module-first', 'module-first')
    command('init', 'module', '--language', 'rust', '-m', variant, '-d', 'postgresql', path=root / 'module-first')
    assert (root / 'module-first/src/iam/main.rs').is_file()
    run('cargo', 'metadata', '--manifest-path', root / 'module-first/Cargo.toml', '--no-deps', '--format-version', '1', cwd=root)
    raw(app, 'auth-runtime')
    command('init', 'module', 'dry-iam', '--language', 'rust', '-m', variant, '--dryrun')
    assert not (app / 'src/dry-iam').exists()
    if not better_auth:
        before = (app / '.forklaunch/manifest.toml').read_bytes()
        command('init', 'module', '--language', 'rust', '-m', 'unsupported-module', failure=True)
        assert (app / '.forklaunch/manifest.toml').read_bytes() == before
        command('init', 'module', '--language', 'rust', '-m', 'iam-base', '-d', 'sqlite', failure=True)
        assert (app / '.forklaunch/manifest.toml').read_bytes() == before
        assert not (app / 'src/iam').exists()
    command('init', 'module', '--language', 'rust', '-m', variant, '-d', 'postgresql')
    relocated = root / 'relocated-app'
    app.rename(relocated)
    app = relocated
    # Build against this checkout's crates (the seed's directory records are
    # not on crates.io until the next crates release).
    run('python3', repo / 'cli/tests/fixtures/local_crates_patch.py', app, repo)
    service = app / 'src/iam'
    assert (app / 'src/iam/sdk/rust/.forklaunch-sdk-local').is_file()
    assert (app / 'src/iam/.forklaunch/contracts/Cargo.toml').is_file()
    assert not (app / 'src/client-sdk/typescript/iam').exists()
    if better_auth:
        assert 'BETTER_AUTH_TWO_FACTOR_ENABLED=false' in (service / '.env.example').read_text()
        assert '${IAM_BETTER_AUTH_TWO_FACTOR_ENABLED:-false}' in (app / 'docker-compose.yaml').read_text()
    else:
        registrations = service / 'registrations.rs'
        source = registrations.read_text()
        # The generated registrations are formatted, so match the call by its tokens.
        old = re.search(r'let iam = IamHttp::new\(\s*sessions,\s*directory,\s*config\.module_auth\(\)\?,\s*config\.directory_mode,\s*service_info\(\),?\s*\);', source)
        assert old, 'generated IAM handler registration'
        old = old.group(0)
        replacement = '''#[allow(dead_code)]
    #[derive(forklaunch_core::persistence::compliance::ComplianceMetadata)]
    #[compliance(entity = "User")]
    struct ClassifiedUser {
        #[compliance(level = "none")]
        id: String,
        #[compliance(level = "pii")]
        email: String,
        #[compliance(level = "phi", name = "firstName")]
        first_name: String,
    }
    let mut compliance = forklaunch_iam_base::directory::compliance::DirectoryCompliance::default();
    if std::env::var("FORKLAUNCH_FIXTURE_CLASSIFIED_IAM").as_deref() == Ok("1") {
        compliance = compliance.with_entity::<ClassifiedUser>(forklaunch_iam_base::directory::DirectoryKind::User, "id")?;
    }
    let iam = IamHttp::new(sessions, directory, config.module_auth()?, config.directory_mode, service_info()).with_compliance(compliance);'''
        registrations.write_text(source.replace(old, replacement, 1))
    library = service / 'lib.rs'
    library.write_text(library.read_text() + '\npub const USER_OWNED_MARKER: &str = "preserved";\n')
    command('sync', 'service', 'iam')
    assert 'USER_OWNED_MARKER' in library.read_text()
    manifest = service / 'Cargo.toml'
    manifest.write_text(manifest.read_text() + '\n[package.metadata.user]\nmarker = "preserved"\n')
    command('init', 'module', '--language', 'rust', '-m', variant, '-d', 'postgresql')
    assert 'USER_OWNED_MARKER' in library.read_text()
    assert 'marker = "preserved"' in manifest.read_text()
    if better_auth:
        assert 'BETTER_AUTH_SECRET' in (app / 'docker-compose.yaml').read_text()
        assert '${IAM_BETTER_AUTH_TWO_FACTOR_ENABLED:-false}' in (app / 'docker-compose.yaml').read_text()
        assert 'BETTER_AUTH_TWO_FACTOR_ENABLED=false' in (service / '.env.example').read_text()
    else:
        assert 'PostgresIamService' in (service / 'registrations.rs').read_text()
    run('cargo', 'build', '--workspace')
    command('openapi', 'export')
    command('sdk', 'generate', '--target', 'all', '--from', 'contract')
    runtime = root / 'typescript-runtime'
    entry = build_public_runtime(run, app, runtime, repo)
    env.setdefault('TSC_BIN', str(runtime / 'node_modules/.bin/tsc'))
    env.setdefault('FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES', str(runtime))
    usage = app / 'iam-typecheck.ts'
    if better_auth:
        body = '''const discovery = await sdk.discovery.getauthmethods();
 if (discovery.code === 200) { const email:boolean = discovery.response.email; const providers:string[] = discovery.response.socialProviders; void email; void providers; }
 const response = await sdk.user.surfaceScopedUserRoles({params:{id:'user',organizationId:'organization',sessionId:'session'}});
 if (response.code === 200) { const name:string = response.response[0].name; void name; }
 // @ts-expect-error The user identifier is a string.
 await sdk.user.surfaceScopedUserRoles({params:{id:7,organizationId:'organization',sessionId:'session'}});'''
    else:
        body = '''const page = await sdk.users.listUsers({query:{limit:1}});
 if(page.code===200) { const email:string=page.response.items[0].email; const nullable:typeof page.response.next_cursor=null; void email; void nullable; }
 const user = await sdk.user.getUser({params:{id:'00000000-0000-4000-8000-000000000000'}});
 if(user.code===200) { const slug:string=user.response.roles[0].permissions[0].slug; void slug; }
 const updated=await sdk.user.updateUser({body:{id:'00000000-0000-4000-8000-000000000000',firstName:'Typed'}});
 if(updated.code===200) { const message:string=updated.response; void message; }
 // @ts-expect-error Credentials require a string password.
 await sdk.auth.authenticate({body:{email:'test@example.com',password:1}});'''
    usage.write_text("import {universalSdk} from '@forklaunch/universal-sdk';\nimport type { IamSdkClient } from './src/iam/sdk/typescript/index.js';\nasync function check(){const sdk=await universalSdk<IamSdkClient>({host:'https://iam.example',registryOptions:{path:'openapi.json'}});\n" + body + '\n}\nvoid check;\n')
    typecheck(run, app, runtime, [app / 'src/iam/sdk/typescript/index.ts', usage])
    source = None if better_auth else (repo / 'cli/tests/fixtures/rust_iam_sdk_runtime.rs').read_text()
    consumer = rust_consumer(app, root / 'rust-consumer', 'iam', package='iam-sdk', test_source=source)
    run('cargo', 'check', '--manifest-path', consumer, '--all-targets')
    if better_auth:
        if not env.get('FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES'):
            dependencies = root / 'better-auth-client'
            run('npm', 'install', '--prefix', dependencies, '--no-save', '--package-lock=false', '--ignore-scripts', 'better-auth@1.7.6')
            env['FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES'] = str(dependencies)
        client = root / 'typescript-client'
        raw(client, 'auth-client')
        command('init', 'module', '--language', 'typescript', '-m', 'iam-better-auth', '-d', 'postgresql', path=client)
        env['FORKLAUNCH_TEST_GENERATED_CLIENT_SDK'] = str(client / 'src/client-sdk/clientSdk.ts')
        assert Path(env['FORKLAUNCH_TEST_GENERATED_CLIENT_SDK']).is_file()
        for factory in [repo / 'blueprint/client-sdk/clientSdk.ts',
                        Path(env['FORKLAUNCH_TEST_GENERATED_CLIENT_SDK'])]:
            run('node', repo / 'cli/tests/rust_better_auth_frontend_composition.mjs',
                extra_env={'FORKLAUNCH_TEST_GENERATED_CLIENT_SDK': str(factory)})
    database.start()
    run('cargo', 'run', '-p', 'iam', '--', '--migrate', extra_env={'DATABASE_URL': env['FORKLAUNCH_TEST_POSTGRES_URL']})
    (service / 'examples').mkdir(exist_ok=True)
    seed = 'rust_better_auth_seed.rs' if better_auth else 'rust_iam_seed.rs'
    shutil.copy2(repo / 'cli/tests/fixtures' / seed, service / 'examples/seed.rs')
    role_created = True
    run('cargo', 'run', '-p', 'iam', '--example', 'seed')
    if not better_auth:
        # `iam --seed` (generated seed.toml) is idempotent under the restricted runtime role.
        from urllib.parse import urlsplit, urlunsplit
        owner = urlsplit(env['FORKLAUNCH_TEST_POSTGRES_URL'])
        runtime_url = urlunsplit(owner._replace(netloc=f"{env['FORKLAUNCH_TEST_ROLE']}:parity-test-only@{owner.hostname}:{owner.port or 5432}"))
        seed_env = {'DATABASE_URL': runtime_url, 'TENANT_ID': env['FORKLAUNCH_TEST_ROLE'] + '-a', 'IAM_SEED_ADMIN_PASSWORD': 'seed-admin-password', 'ENCRYPTION_KEY': 'iam-fixture-encryption-only'}
        assert 'name = "admin"' in (service / 'seed.toml').read_text()
        run('cargo', 'run', '-p', 'iam', '--', '--seed', extra_env=seed_env)
        run('cargo', 'run', '-p', 'iam', '--', '--seed', extra_env=seed_env)
        run('cargo', 'run', '-p', 'iam', '--', '--seed', failure=True, extra_env={**seed_env, 'IAM_SEED_ADMIN_PASSWORD': ''})
    binary = Path(env['CARGO_TARGET_DIR']) / 'debug/iam'
    script = 'rust_better_auth_runtime.mjs' if better_auth else 'rust_iam_runtime.mjs'
    run('node', repo / 'cli/tests' / script, binary, entry, consumer)
    lifecycle = root / 'lifecycle-app'
    shutil.copytree(app, lifecycle, symlinks=True)
    occupied = lifecycle / 'src/occupied'
    occupied.mkdir()
    (occupied / 'marker').write_text('user-owned destination\n')
    command('change', 'service', '-N', 'occupied', '--confirm', path=lifecycle / 'src/iam', failure=True)
    assert (occupied / 'marker').read_text() == 'user-owned destination\n'
    assert (lifecycle / 'src/iam/main.rs').is_file()
    command('change', 'service', '-N', 'identity', '--confirm', path=lifecycle / 'src/iam')
    assert not (lifecycle / 'src/iam').exists()
    assert 'USER_OWNED_MARKER' in (lifecycle / 'src/identity/lib.rs').read_text()
    project = next(p for p in tomllib.loads((lifecycle / '.forklaunch/manifest.toml').read_text())['projects'] if p['name'] == 'identity')
    assert project['variant'] == variant
    assert not (lifecycle / 'src/client-sdk/rust/iam.rs').exists()
    assert not (lifecycle / 'src/client-sdk/rust/iam').exists()
    assert (lifecycle / 'src/identity/sdk/rust/Cargo.toml').is_file()
    assert (lifecycle / 'src/identity/.forklaunch/contracts/Cargo.toml').is_file()
    assert project['metadata']['sdkLayout'] == 'project-local-v1'
    contract = json.loads((lifecycle / project['metadata']['contractArtifactPath']).read_text())
    assert contract['service']['name'] == 'identity'
    assert not (lifecycle / '.forklaunch/contracts/iam').exists()
    barrel = (lifecycle / 'src/client-sdk/index.ts').read_text()
    assert 'IdentitySdkClient' in barrel and 'IamSdkClient' not in barrel
    command('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check', path=lifecycle)
    run('cargo', 'check', '--manifest-path', lifecycle / 'Cargo.toml', '--workspace')
    command('delete', 'service', 'identity', '--continue', path=lifecycle)
    assert not (lifecycle / 'src/identity').exists()
    print('PASS raw ' + variant + ' module, real PostgreSQL security/session/HTTP/MCP runtime, public TS and Rust SDKs, preservation, relocation and lifecycle', flush=True)
    receipt = env.get('FORKLAUNCH_BETTER_AUTH_RECEIPT' if better_auth else 'FORKLAUNCH_IAM_RECEIPT')
    if receipt:
        Path(receipt).write_text(json.dumps({'status':'PASS','cliSha256':digest,'variant':variant,
            'registryDiscovery':{'path':'openapi.json','convention':'Relative to host, matching blueprint/iam-base/surfacing.ts and blueprint/iam-better-auth/surfacing.ts; actual hash/document discovery remains enabled'},
            'runtime':'Original database, authentication, authorization, encrypted session, HTTP/MCP and shutdown assertions'},indent=2)+'\n')
    passed = True
finally:
    if role_created:
        subprocess.run(['cargo','run','--quiet','--manifest-path',str(app / 'Cargo.toml'),'-p','iam','--example','seed','--','--cleanup'],
                       cwd=app, env=env, check=False, timeout=1800, stdout=subprocess.DEVNULL)
    database.close()
    if passed and not env.get('FORKLAUNCH_KEEP_FIXTURE'):
        shutil.rmtree(root)
    else:
        print('Fixture retained:', root, flush=True)
